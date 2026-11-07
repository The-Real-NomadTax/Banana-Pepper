package com.bananapepper

import android.content.Context
import org.json.JSONArray
import org.json.JSONObject
import java.io.File

object RustBackend {
    init { System.loadLibrary("banana_ffi") }
    @JvmStatic private external fun request(operation: String, payload: String): String
    fun call(operation: String, payload: JSONObject): Any {
        val response = JSONObject(request(operation, payload.toString()))
        check(response.getBoolean("ok")) { response.optString("error", "Rust backend failed") }
        return response.get("value")
    }
    fun catalog(context: Context): JSONArray {
        val prefs = context.getSharedPreferences("library", Context.MODE_PRIVATE)
        val sources = JSONObject(prefs.getString("sources", "{}")!!)
        val roots = JSONArray().put(File(context.filesDir, "games").path)
        context.getExternalFilesDir(null)?.let { roots.put(File(it, "games").path) }
        return call("catalog", JSONObject().put("files", context.filesDir.path).put("roots", roots).put("sources", sources)) as JSONArray
    }
    fun saveSource(context: Context, id: String, path: String) {
        val prefs = context.getSharedPreferences("library", Context.MODE_PRIVATE)
        val sources = JSONObject(prefs.getString("sources", "{}")!!).put(id, path)
        prefs.edit().putString("sources", sources.toString()).apply()
    }
}

object Paths {
    fun bin(c: Context) = File(c.filesDir, "bin")
    fun rootfs(c: Context) = File(c.filesDir, "rootfs")
    fun opt(c: Context) = File(c.filesDir, "opt")
    fun run(c: Context) = File(c.filesDir, "run")
}
