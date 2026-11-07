package com.bananapepper

import org.json.JSONObject
import java.io.File

/** ADB/run-as diagnostic entrypoint; uses exactly the UI's Rust API and process plan. */
object BackendProbe {
    @JvmStatic fun main(args:Array<String>) {
        require(args.size>=2) { "BackendProbe <catalog|prepare> <request.json> [--run]" }
        val request=JSONObject(File(args[1]).readText())
        request.put("AF_APP_PID",android.os.Process.myPid().toString())
        val value=RustBackend.call(args[0],request)
        println(value.toString())
        if(args.getOrNull(2)=="--run") {
            val plan=value as JSONObject
            val command=plan.getJSONArray("command")
            val arguments=(0 until command.length()).map { command.getString(it) }.toMutableList()
            request.optJSONObject("guest_environment")?.let { overrides ->
                val insert=arguments.indexOf("/bin/bash")
                require(insert>=0)
                val settings=overrides.keys().asSequence().map { key ->
                    require(key.startsWith("BOX64_") && key.all { it.isUpperCase() || it.isDigit() || it=='_' })
                    "$key=${overrides.getString(key)}"
                }.toList()
                arguments.addAll(insert,settings)
                plan.put("command",org.json.JSONArray(arguments))
            }
            val process=GuestProcess.start(plan)
            Runtime.getRuntime().addShutdownHook(Thread { process.stop() })
            val seconds=request.optLong("timeout_seconds",45).coerceIn(1,900)
            if(!process.waitFor(seconds,java.util.concurrent.TimeUnit.SECONDS)) {
                process.stop()
                println("Probe timeout after $seconds seconds")
            } else { println("Guest exit code: ${process.exitValue()}"); process.stop() }
        }
    }
}
