package com.ddadan.player.data

import com.ddadan.player.BuildConfig
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.isActive
import kotlinx.coroutines.joinAll
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import okhttp3.OkHttpClient
import okhttp3.Request
import java.net.Inet4Address
import java.net.NetworkInterface
import java.net.URI
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference
import kotlin.coroutines.coroutineContext

/** 탐색 진행 상황. currentIp = 방금 프로브한 IP, done/total = 진행/전체. */
data class ScanProgress(val currentIp: String, val done: Int, val total: Int)

/**
 * 로컬 네트워크에서 DDADAN API 서버를 찾는다.
 *
 * 단순 ICMP ping 대신 API 포트로 실제 HTTP 프로브를 보내, 응답이 우리 서버인지까지 확인한다.
 * (루팅 없는 안드로이드에서 ICMP ping은 신뢰도가 낮고, 공유기/프린터 등도 ping에 응답하기 때문.)
 * 기기 자신의 /24 서브넷을 자동 추출해 host 부분 2~255를 순회한다.
 */
class ServerLocator(
  private val maxParallel: Int = 10,
  private val perHostTimeoutSec: Long = 10,
) {
  private val template: URI = URI(BuildConfig.API_BASE)

  private val json = Json { ignoreUnknownKeys = true; isLenient = true }

  /** 프로브용 OkHttp 클라이언트. 호스트당 connect/read 타임아웃을 짧게 잡는다. */
  private val client: OkHttpClient =
    OkHttpClient.Builder()
      .connectTimeout(perHostTimeoutSec, TimeUnit.SECONDS)
      .readTimeout(perHostTimeoutSec, TimeUnit.SECONDS)
      .callTimeout(perHostTimeoutSec, TimeUnit.SECONDS)
      .retryOnConnectionFailure(false)
      .build()

  /**
   * 서브넷을 한 바퀴 스캔한다. 서버를 찾으면 그 base 주소(예: http://192.168.45.2:7800/api)를 반환,
   * 못 찾으면 null. onProgress로 현재 탐색 IP와 진행률을 흘려보낸다.
   */
  suspend fun scanOnce(onProgress: (ScanProgress) -> Unit): String? =
    coroutineScope {
      val prefix = localSubnetPrefix() ?: return@coroutineScope null
      val ownIp = localIpv4()
      val candidates = (2..255).map { "$prefix.$it" }.filter { it != ownIp }
      val total = candidates.size
      val started = AtomicInteger(0)
      val found = AtomicReference<String?>(null)
      val semaphore = Semaphore(maxParallel)

      val jobs =
        candidates.map { ip ->
          launch(Dispatchers.IO) {
            semaphore.withPermit {
              if (found.get() != null || !coroutineContext.isActive) return@withPermit
              onProgress(ScanProgress(ip, started.incrementAndGet(), total))
              val base = candidateBase(ip)
              if (probe(base)) {
                found.compareAndSet(null, base)
              }
            }
          }
        }
      jobs.joinAll()
      found.get()
    }

  /**
   * candidate base의 health 엔드포인트(/api/health/live)를 호출해 우리 서버인지 검증한다.
   * 해당 경로에서 200 + {"status":"ok"}를 받을 때만 우리 서버로 판정한다.
   * (임의 경로에 404를 주는 다른 웹서버는 걸러지고, 기기 등록 여부와 무관하게 동작한다.)
   */
  private fun probe(base: String): Boolean =
    try {
      val url = "$base/health/live"
      val request = Request.Builder().url(url).get().build()
      client.newCall(request).execute().use { resp ->
        if (!resp.isSuccessful) {
          false
        } else {
          val body = resp.body?.string()
          if (body.isNullOrBlank()) {
            false
          } else {
            json.decodeFromString(HealthPing.serializer(), body).status == "ok"
          }
        }
      }
    } catch (_: Exception) {
      false
    }

  @Serializable
  private data class HealthPing(val status: String)

  /** 빌드 기본 API_BASE의 scheme/port/path는 유지하고 host만 candidate IP로 치환. */
  private fun candidateBase(ip: String): String {
    val scheme = template.scheme ?: "http"
    val portPart = if (template.port != -1) ":${template.port}" else ""
    val path = template.path?.trimEnd('/').orEmpty()
    return "$scheme://$ip$portPart$path"
  }

  /** 자기 IPv4의 앞 3옥텟(예: "192.168.45"). site-local IPv4를 우선 선택. */
  private fun localSubnetPrefix(): String? = localIpv4()?.substringBeforeLast('.')

  private fun localIpv4(): String? {
    val interfaces =
      try {
        NetworkInterface.getNetworkInterfaces()?.toList().orEmpty()
      } catch (_: Exception) {
        return null
      }
    for (nif in interfaces) {
      val up = try { nif.isUp && !nif.isLoopback } catch (_: Exception) { false }
      if (!up) continue
      for (addr in nif.inetAddresses) {
        if (addr is Inet4Address && addr.isSiteLocalAddress) {
          return addr.hostAddress
        }
      }
    }
    return null
  }
}
