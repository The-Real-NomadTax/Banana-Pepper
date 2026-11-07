package com.bananapepper

import android.system.Os
import android.system.OsConstants
import org.json.JSONObject
import java.io.File
import java.util.concurrent.TimeUnit

/** Own one Linux session, including children left behind by a crashed game. */
class GuestProcess private constructor(private val child: Process, private val pidFile: File) {
    private data class Tracee(val pid:Int,val started:String)
    private var stopped=false
    private fun identity(pid:Int):String? = runCatching {
        val fields=File("/proc/$pid/stat").readText().substringAfterLast(") ").split(' ')
        if(fields[0]=="Z")null else fields[19]
    }.getOrNull()
    private fun tracees(tracer:Int):List<Tracee> = File("/proc").listFiles().orEmpty().mapNotNull { directory ->
        val pid=directory.name.toIntOrNull()?:return@mapNotNull null
        runCatching {
            if(Os.stat(directory.path).st_uid!=android.os.Process.myUid())return@runCatching null
            val attached=File(directory,"status").useLines { lines ->
                lines.any { it.startsWith("TracerPid:") && it.substringAfter(':').trim()==tracer.toString() }
            }
            if(attached)identity(pid)?.let { Tracee(pid,it) } else null
        }.getOrNull()
    }
    private fun signal(tracee:Tracee,signal:Int) {
        if(identity(tracee.pid)==tracee.started)runCatching { Os.kill(tracee.pid,signal) }
    }
    val isAlive get() = child.isAlive
    fun exitValue() = child.exitValue()
    fun waitFor(seconds: Long, unit: TimeUnit) = child.waitFor(seconds,unit)
    @Synchronized fun stop() {
        if(stopped)return
        stopped=true
        var pid: Int? = null
        for(i in 0 until 10) {
            pid=runCatching { pidFile.readText().trim().toIntOrNull() }.getOrNull()
            if(pid!=null || !child.isAlive)break
            Thread.sleep(10)
        }
        val group=pid?.takeIf { it>1 }
        // Wine creates additional sessions. Track proot's actual tracees as well
        // as its process group, while the tracer still identifies them.
        val attached=group?.let { tracees(it) }.orEmpty()
        attached.forEach { signal(it,OsConstants.SIGTERM) }
        if(group!=null)runCatching { Os.kill(-group,OsConstants.SIGTERM) }
        child.destroy()
        val deadline=System.nanoTime()+TimeUnit.SECONDS.toNanos(2)
        while(group!=null && System.nanoTime()<deadline) {
            if(runCatching { Os.kill(-group,0) }.isFailure && attached.none { identity(it.pid)==it.started })break
            Thread.sleep(50)
        }
        attached.forEach { signal(it,OsConstants.SIGKILL) }
        if(group!=null)runCatching { Os.kill(-group,OsConstants.SIGKILL) }
        if(child.isAlive)child.destroyForcibly()
        child.waitFor(2,TimeUnit.SECONDS)
    }
    companion object {
        fun start(plan: JSONObject): GuestProcess {
            val run=File(plan.getString("run"))
            val pid=File(run,"host-session.pid")
            pid.delete()
            val command=plan.getJSONArray("command")
            // The shell writes its real Android PID before exec enters proot.
            // Dynamic paths and argv are separate arguments, never shell text.
            val arguments=mutableListOf("/system/bin/toybox","setsid","/system/bin/sh","-c",
                "printf '%s\\n' \"\$\$\" > \"\$1\"; shift; exec \"\$@\"","banana-guest",pid.path)
            for(i in 0 until command.length())arguments.add(command.getString(i))
            val builder=ProcessBuilder(arguments)
            builder.environment().apply {
                remove("LD_PRELOAD")
                val environment=plan.getJSONObject("environment")
                for(key in environment.keys())put(key,environment.getString(key))
            }
            File(run,"launch.json").writeText(plan.toString(2))
            builder.redirectErrorStream(true).redirectOutput(File(run,"session.log"))
            return GuestProcess(builder.start(),pid)
        }
    }
}
