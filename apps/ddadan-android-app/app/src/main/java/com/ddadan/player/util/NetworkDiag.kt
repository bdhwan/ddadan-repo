package com.ddadan.player.util

import android.content.Context
import android.net.wifi.WifiManager
import java.net.Inet4Address
import java.net.NetworkInterface

/** 서버 탐색 실패 원인 파악용 기기 네트워크 진단 정보. */
data class NetDiag(
  /** 비루프백 IPv4 주소들. "192.168.150.7 (wlan0)" 형태. 비어있으면 네트워크 미연결. */
  val ips: List<String>,
  /** 연결됨 / 미연결 / 꺼짐 */
  val wifiState: String,
  val ssid: String?,
  val gateway: String?,
) {
  /** 스캔 대상 서브넷 "192.168.150.x" (첫 IPv4 기준). */
  val subnet: String?
    get() =
      ips.firstOrNull()?.substringBefore(" ")?.let { ip ->
        ip.split(".").takeIf { it.size == 4 }?.let { "${it[0]}.${it[1]}.${it[2]}.x" }
      }
}

fun gatherNetDiag(context: Context): NetDiag {
  val ips =
    try {
      NetworkInterface.getNetworkInterfaces().toList().flatMap { nif ->
        nif.inetAddresses.toList()
          .filter { !it.isLoopbackAddress && it is Inet4Address }
          .map { "${it.hostAddress} (${nif.name})" }
      }
    } catch (e: Exception) {
      emptyList()
    }

  var wifiState = "알 수 없음"
  var ssid: String? = null
  var gateway: String? = null
  try {
    val wm = context.applicationContext.getSystemService(Context.WIFI_SERVICE) as WifiManager
    @Suppress("DEPRECATION")
    val info = wm.connectionInfo
    ssid =
      info?.ssid
        ?.trim('"')
        ?.takeIf { it.isNotBlank() && it != "<unknown ssid>" && it != "0x" }
    wifiState =
      when {
        !wm.isWifiEnabled -> "꺼짐"
        ssid != null && info.networkId != -1 -> "연결됨"
        else -> "미연결"
      }
    @Suppress("DEPRECATION")
    val gw = wm.dhcpInfo?.gateway ?: 0
    if (gw != 0) {
      gateway =
        String.format(
          "%d.%d.%d.%d",
          gw and 0xff,
          (gw shr 8) and 0xff,
          (gw shr 16) and 0xff,
          (gw shr 24) and 0xff,
        )
    }
  } catch (e: Exception) {
    // 권한/기기 이슈 시 부분 정보만.
  }

  return NetDiag(ips = ips, wifiState = wifiState, ssid = ssid, gateway = gateway)
}
