#!/usr/bin/env python3
"""로컬 포트를 원격 호스트로 넘기는 의존성 없는 TCP relay.

용도: Tailscale 을 못 쓰는 기기(스마트TV, 일부 태블릿, 로컬 IP 만 넣을 수 있는 앱)를
사무실 사이니지 서버에 붙이기. 집처럼 그 기기와 같은 LAN 에 있는 상시 기기 한 대에
이 relay 를 띄우면, 그 기기의 로컬 IP:포트 로 들어온 연결이 Tailscale 너머의
서버로 넘어간다.

  [집] 스마트TV ──http://192.168.0.246:7300──▶ [집 맥] relay ──Tailscale──▶ [사무실] display-4:7300

사이니지 웹(admin/player)은 자기 오리진의 /api·/static 을 nginx 가 내부 프록시하므로,
포트별 단순 TCP relay 만으로 CORS 문제 없이 그대로 동작한다.

표준 라이브러리만 사용 — Python 3.6+ 면 설치 없이 실행된다.

예:
  # display-4(Tailscale 100.96.152.109)의 player(7300)+admin(4200)을 이 기기로 노출
  ./ddadan-relay.py --dest 100.96.152.109 --forward 7300 --forward 4200

  # 로컬 9000 을 원격 7300 으로 (포트 번호를 바꿔서 노출)
  ./ddadan-relay.py --dest 100.96.152.109 --forward 9000:7300

부팅 자동실행이 필요하면 OS 서비스로 감싼다(리눅스 systemd / macOS launchd).
자세한 건 scripts/relay/README.md 참고.
"""
import argparse
import socket
import sys
import threading

BUF = 65536


def pipe(src, dst):
    """src 에서 읽어 dst 로 쓴다. 한쪽이 닫히면 양쪽을 정리한다."""
    try:
        while True:
            data = src.recv(BUF)
            if not data:
                break
            dst.sendall(data)
    except OSError:
        pass
    finally:
        for s in (src, dst):
            try:
                s.shutdown(socket.SHUT_RDWR)
            except OSError:
                pass
            try:
                s.close()
            except OSError:
                pass


def handle(client, dest_host, dest_port):
    try:
        remote = socket.create_connection((dest_host, dest_port), timeout=10)
    except OSError as e:
        sys.stderr.write(f"[relay] {dest_host}:{dest_port} 연결 실패: {e}\n")
        client.close()
        return
    threading.Thread(target=pipe, args=(client, remote), daemon=True).start()
    threading.Thread(target=pipe, args=(remote, client), daemon=True).start()


def listen(bind_host, local_port, dest_host, dest_port):
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind((bind_host, local_port))
    except OSError as e:
        sys.stderr.write(f"[relay] :{local_port} bind 실패: {e}\n")
        return
    s.listen(128)
    sys.stderr.write(f"[relay] {bind_host}:{local_port} -> {dest_host}:{dest_port}\n")
    while True:
        client, _ = s.accept()
        threading.Thread(
            target=handle, args=(client, dest_host, dest_port), daemon=True
        ).start()


def parse_forward(spec):
    """'7300' -> (7300, 7300),  '9000:7300' -> (9000, 7300)."""
    if ":" in spec:
        lp, rp = spec.split(":", 1)
    else:
        lp = rp = spec
    try:
        return int(lp), int(rp)
    except ValueError:
        raise argparse.ArgumentTypeError(f"잘못된 --forward 값: {spec!r} (예: 7300 또는 9000:7300)")


def main(argv=None):
    ap = argparse.ArgumentParser(
        description="로컬 포트를 원격 호스트로 넘기는 TCP relay",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="예:\n  %(prog)s --dest 100.96.152.109 --forward 7300 --forward 4200\n",
    )
    ap.add_argument("--dest", required=True, help="넘길 대상 호스트(예: Tailscale IP)")
    ap.add_argument(
        "--forward",
        required=True,
        action="append",
        type=parse_forward,
        metavar="LOCAL[:REMOTE]",
        help="포워딩할 포트. REMOTE 생략 시 LOCAL 과 동일. 여러 번 지정 가능.",
    )
    ap.add_argument(
        "--bind",
        default="0.0.0.0",
        help="리스닝 주소 (기본 0.0.0.0 = 모든 인터페이스; 127.0.0.1 로 로컬 전용)",
    )
    args = ap.parse_args(argv)

    for local_port, remote_port in args.forward:
        threading.Thread(
            target=listen,
            args=(args.bind, local_port, args.dest, remote_port),
            daemon=True,
        ).start()

    sys.stderr.write("[relay] up. Ctrl-C 또는 kill 로 종료.\n")
    sys.stderr.flush()
    try:
        threading.Event().wait()
    except KeyboardInterrupt:
        sys.stderr.write("\n[relay] 종료.\n")


if __name__ == "__main__":
    main()
