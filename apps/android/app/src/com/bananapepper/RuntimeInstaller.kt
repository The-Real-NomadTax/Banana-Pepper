package com.bananapepper

import android.content.Context
import android.system.Os
import android.system.OsConstants
import java.io.*
import java.util.zip.GZIPInputStream

/** Runtime installation is platform plumbing; title preparation lives in Rust. */
object RuntimeInstaller {
    private const val MARKER = ".arcade-runtime-version"
    private fun version(c: Context) = c.assets.open("runtime/version.txt").bufferedReader().use { it.readText().trim() }
    private fun toolsVersion(c: Context) = c.assets.open("runtime/tools-version.txt").bufferedReader().use { it.readText().trim() }
    fun installed(c: Context): Boolean = try {
        File(Paths.rootfs(c), MARKER).readText().trim() == version(c) &&
            File(Paths.bin(c), MARKER).readText().trim() == "${version(c)}/${toolsVersion(c)}"
    } catch (_: Exception) { false }
    fun install(c: Context, progress: (String, Int) -> Unit) {
        val version = version(c)
        val root = Paths.rootfs(c)
        if (!File(root, MARKER).let { it.isFile && it.readText().trim() == version }) {
            val staging = File(c.filesDir, "rootfs.installing")
            deleteTree(staging); staging.mkdirs()
            progress("Unpacking game runtime…", 0)
            try {
                GZIPInputStream(c.assets.open("runtime/rootfs.tar.gz").buffered(65536), 65536).use {
                    extractTar(it, staging) { progress("Unpacking game runtime…", -1) }
                }
                File(staging, MARKER).writeText(version)
                val previous = File(c.filesDir, "rootfs.previous")
                deleteTree(previous)
                if (root.exists()) check(root.renameTo(previous)) { "Cannot retain previous runtime" }
                if (!staging.renameTo(root)) {
                    previous.renameTo(root)
                    error("Cannot activate runtime")
                }
                deleteTree(previous)
            } catch (e: Exception) { deleteTree(staging); throw e }
        }
        progress("Updating runtime tools…", 95)
        Paths.bin(c).mkdirs(); Paths.opt(c).mkdirs()
        copyAsset(c, "runtime/banana-tracer", File(Paths.bin(c), "banana-tracer"), 493)
        // The PRoot runtime it replaces (tracer, loader and their libraries).
        for (old in listOf("proot", "loader", "libtalloc.so.2", "libandroid-shmem.so"))
            File(Paths.bin(c), old).delete()
        val tools = Paths.opt(c)
        val stagedTools = File(c.filesDir,"opt.installing")
        val previousTools = File(c.filesDir,"opt.previous")
        deleteTree(stagedTools)
        try {
            installAssets(c,"runtime/tools",stagedTools)
            // Wine uses wine.inf's timestamp to detect a prefix update. Keep it
            // stable when only the UI, game adapter, or another tool changed.
            val previousInf=File(tools,"wine/share/wine/wine.inf")
            val stagedInf=File(stagedTools,"wine/share/wine/wine.inf")
            if(previousInf.isFile && stagedInf.isFile && previousInf.length()==stagedInf.length()
                && previousInf.readBytes().contentEquals(stagedInf.readBytes())) {
                check(stagedInf.setLastModified(previousInf.lastModified())) { "Cannot retain Wine runtime timestamp" }
            }
            File(stagedTools,".banana-tools-ready").writeText(version)
            deleteTree(previousTools)
            if(tools.exists())check(tools.renameTo(previousTools)) { "Cannot retain previous runtime tools" }
            if(!stagedTools.renameTo(tools)) {
                previousTools.renameTo(tools)
                error("Cannot activate runtime tools")
            }
            File(Paths.bin(c), MARKER).writeText("$version/${toolsVersion(c)}")
            deleteTree(previousTools)
        } catch(e:Exception) { deleteTree(stagedTools); throw e }
        progress("Runtime ready", 100)
    }
    private fun installAssets(c: Context, path: String, dest: File) {
        val children = c.assets.list(path) ?: emptyArray()
        if (children.isEmpty()) { copyAsset(c, path, dest, 493); return }
        dest.mkdirs()
        for (name in children) installAssets(c, "$path/$name", File(dest,name))
    }
    private fun copyAsset(c: Context, asset: String, dest: File, mode: Int) {
        val tmp = File(dest.path + ".installing")
        c.assets.open(asset).use { input -> tmp.outputStream().use { input.copyTo(it,131072) } }
        Os.chmod(tmp.path, mode)
        check(tmp.renameTo(dest)) { "Cannot install ${dest.name}" }
    }
    fun deleteTree(f: File) {
        if (isLink(f)) { check(f.delete()) { "Cannot remove link" }; return }
        if (f.isDirectory) f.listFiles()?.forEach { deleteTree(it) }
        if (f.exists()) check(f.delete()) { "Cannot remove ${f.name}" }
    }
    private fun isLink(f: File) = try { OsConstants.S_ISLNK(Os.lstat(f.path).st_mode) } catch (_: Exception) { false }
    private fun exact(input: InputStream, data: ByteArray): Boolean {
        var n = 0
        while (n < data.size) {
            val count = input.read(data, n, data.size - n)
            if (count < 0) { if (n == 0) return false; error("Truncated runtime archive") }
            n += count
        }
        return true
    }
    private fun skip(input: InputStream, count: Long) {
        var left = count
        val buffer = ByteArray(8192)
        while (left > 0) {
            val n = input.read(buffer, 0, minOf(buffer.size.toLong(), left).toInt())
            check(n > 0) { "Truncated runtime archive" }; left -= n
        }
    }
    private fun str(b: ByteArray, offset: Int, length: Int) = b.copyOfRange(offset, offset + length).toString(Charsets.UTF_8).substringBefore('\u0000')
    private fun number(b: ByteArray, offset: Int, length: Int): Long {
        if (b[offset].toInt() and 128 != 0) {
            var n = 0L
            for (i in offset+1 until offset+length) n = (n shl 8) or (b[i].toLong() and 255)
            return n
        }
        return str(b, offset, length).trim().ifEmpty { "0" }.toLong(8)
    }
    private fun extractTar(input: InputStream, root: File, tick: () -> Unit) {
        val header = ByteArray(512)
        var longName: String? = null; var longLink: String? = null; var entries = 0
        val canonicalRoot = root.canonicalPath + "/"
        while (exact(input, header) && header.any { it != 0.toByte() }) {
            val prefix = str(header,345,155)
            val name = (longName ?: ((if (prefix.isEmpty()) "" else "$prefix/") + str(header,0,100))).removePrefix("./")
            val link = longLink ?: str(header,157,100)
            longName = null; longLink = null
            val type = header[156].toInt().toChar(); val size = number(header,124,12)
            val mode = number(header,100,8).toInt() and 511
            val pad = (512 - size % 512) % 512
            if (type in listOf('L','K','x','g')) {
                check(size in 0..1048576) { "Invalid tar metadata" }
                val data = ByteArray(size.toInt()); check(exact(input,data)); skip(input,pad)
                val value = data.toString(Charsets.UTF_8).trimEnd('\u0000')
                if (type == 'L') longName = value
                if (type == 'K') longLink = value
                if (type == 'x') {
                    for (line in value.lines()) {
                        val record = line.substringAfter(' ', "")
                        if (record.startsWith("path=")) longName = record.substringAfter('=')
                        if (record.startsWith("linkpath=")) longLink = record.substringAfter('=')
                    }
                }
                continue
            }
            if (name.isEmpty() || name == ".") { skip(input,size+pad); continue }
            check(!name.startsWith('/') && name.split('/').none { it == ".." }) { "Unsafe runtime path" }
            val target = File(root,name)
            check(target.parentFile!!.canonicalPath.let { it == root.canonicalPath || it.startsWith(canonicalRoot) }) { "Unsafe runtime link" }
            target.parentFile!!.mkdirs()
            when (type) {
                '5' -> { target.mkdirs(); Os.chmod(target.path, mode or 448) }
                '2' -> { target.delete(); Os.symlink(link,target.path) }
                '1' -> {
                    val source = File(root,link.removePrefix("./"))
                    check(source.canonicalPath.startsWith(canonicalRoot)) { "Unsafe hard link" }
                    source.inputStream().use { i -> target.outputStream().use { i.copyTo(it) } }
                    Os.chmod(target.path,mode or 384)
                }
                '0','\u0000','7' -> {
                    check(!isLink(target)) { "Unsafe archive entry" }
                    target.outputStream().use { out ->
                        var left = size; val buffer = ByteArray(65536)
                        while (left > 0) {
                            val n = input.read(buffer,0,minOf(left,buffer.size.toLong()).toInt())
                            check(n>0) { "Truncated runtime archive" }; out.write(buffer,0,n); left-=n
                        }
                    }
                    skip(input,pad); Os.chmod(target.path,mode or 384)
                }
                else -> skip(input,size+pad)
            }
            if (++entries % 256 == 0) tick()
        }
    }
}
