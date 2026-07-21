package com.ddadan.watchdog

import android.app.ActivityManager
import android.content.Context
import android.os.Environment
import android.os.StatFs
import com.ddadan.core.TelemetryBody
import java.io.RandomAccessFile

/** CPU/RAM/디스크 자원 수집. */
object Telemetry {
  fun gather(context: Context): TelemetryBody {
    val am = context.getSystemService(Context.ACTIVITY_SERVICE) as ActivityManager
    val mem = ActivityManager.MemoryInfo().also { am.getMemoryInfo(it) }
    val ramTotalMb = mem.totalMem / (1024 * 1024)
    val ramUsedMb = (mem.totalMem - mem.availMem) / (1024 * 1024)

    val stat = StatFs(Environment.getDataDirectory().path)
    val total = stat.blockCountLong * stat.blockSizeLong
    val free = stat.availableBlocksLong * stat.blockSizeLong
    val diskUsedPercent = if (total > 0) (total - free) * 100.0 / total else null

    return TelemetryBody(
      appVersion = BuildConfig.VERSION_NAME,
      cpuPercent = readCpuPercent(),
      ramUsedMb = ramUsedMb,
      ramTotalMb = ramTotalMb,
      diskUsedPercent = diskUsedPercent?.let { Math.round(it * 10) / 10.0 },
    )
  }

  /** /proc/stat 두 번 샘플링해 전체 CPU 사용률(%) 계산. 실패 시 null. */
  private fun readCpuPercent(): Double? =
    try {
      val (idle1, total1) = readCpuSnapshot()
      Thread.sleep(200)
      val (idle2, total2) = readCpuSnapshot()
      val dTotal = (total2 - total1).toDouble()
      val dIdle = (idle2 - idle1).toDouble()
      if (dTotal <= 0) null else Math.round((1.0 - dIdle / dTotal) * 1000) / 10.0
    } catch (_: Exception) {
      null
    }

  private fun readCpuSnapshot(): Pair<Long, Long> {
    RandomAccessFile("/proc/stat", "r").use { raf ->
      val parts = raf.readLine().split(" ").filter { it.isNotBlank() }
      // cpu user nice system idle iowait irq softirq ...
      val nums = parts.drop(1).map { it.toLong() }
      val idle = nums.getOrElse(3) { 0 } + nums.getOrElse(4) { 0 }
      val total = nums.sum()
      return idle to total
    }
  }
}
