package com.ddadan.core

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
 * 로컬 네트워크에서 DDADAN API 서버를 찾는다(플레이어 ServerLocator와 동일 로직).
 * 기기 자신의 /24 서브넷을 자동 추출해 host 2~255를 `/health/live`로 프로브, 200+{"status":"ok"}만 인정.
 */
class ServerLocator(
  private val maxParallel: Int = 10,
  private val perHostTimeoutSec: Long = 10,
) {
  private val template: URI = URI(BuildConfig.API_BASE)
  private val json = Json { ignoreUnknownKeys = true; isLenient = true }

  private val client: OkHttpClient =
    OkHttpClient.Builder()
      .connectTimeout(perHostTimeoutSec, TimeUnit.SECONDS)
      .readTimeout(perHostTimeoutSec, TimeUnit.SECONDS)
      .callTimeout(perHostTimeoutSec, TimeUnit.SECONDS)
      .retryOnConnectionFailure(false)
      .build()

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
              if (probe(base)) found.compareAndSet(null, base)
            }
          }
        }
      jobs.joinAll()
      found.get()
    }

  private fun probe(base: String): Boolean =
    try {
      val request = Request.Builder().url("$base/health/live").get().build()
      client.newCall(request).execute().use { resp ->
        if (!resp.isSuccessful) {
          false
        } else {
          val body = resp.body?.string()
          if (body.isNullOrBlank()) false
          else json.decodeFromString(HealthPing.serializer(), body).status == "ok"
        }
      }
    } catch (_: Exception) {
      false
    }

  private fun candidateBase(ip: String): String {
    val scheme = template.scheme ?: "http"
    val portPart = if (template.port != -1) ":${template.port}" else ""
    val path = template.path?.trimEnd('/').orEmpty()
    return "$scheme://$ip$portPart$path"
  }

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
        if (addr is Inet4Address && addr.isSiteLocalAddress) return addr.hostAddress
      }
    }
    return null
  }

  @Serializable
  private data class HealthPing(val status: String)
}
