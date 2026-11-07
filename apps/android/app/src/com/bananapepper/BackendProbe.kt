package com.bananapepper

import org.json.JSONObject
import java.io.File

/** Diagnostics use the same Rust session API as the UI. */
object BackendProbe {
    @JvmStatic fun main(args:Array<String>) {
        require(args.size>=2) { "BackendProbe <catalog|start> <request.json> [--run]" }
        val request=JSONObject(File(args[1]).readText())
        val value=RustBackend.call(if(args[0]=="prepare") "start" else args[0],request)
        println(value.toString())
        if(args.getOrNull(2)=="--run") {
            val id=(value as JSONObject).getLong("session")
            val session=JSONObject().put("session",id)
            try {
                val end=System.nanoTime()+request.optLong("timeout_seconds",45).coerceIn(1,900)*1000000000L
                while(System.nanoTime()<end) {
                    val state=RustBackend.call("poll",session) as JSONObject
                    if(!state.getBoolean("running")) { println(state.toString()); return }
                    Thread.sleep(100)
                }
                println("Probe timed out")
            } finally { RustBackend.call("stop",session) }
        }
    }
}
