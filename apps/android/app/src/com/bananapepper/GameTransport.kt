package com.bananapepper

import android.content.Context
import android.graphics.*
import android.media.*
import android.net.LocalServerSocket
import android.net.LocalSocket
import android.net.LocalSocketAddress
import android.view.SurfaceHolder
import android.view.SurfaceView
import java.io.*
import java.nio.ByteOrder
import java.nio.MappedByteBuffer
import java.nio.channels.FileChannel
import java.security.SecureRandom
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import org.json.JSONObject

class GameSession(private val context: Context) {
    private val token = java.lang.Long.toHexString(SecureRandom().nextLong())
    val inputSocket = "bananapepper.$token.input"
    val audioSocket = "bananapepper.$token.audio"
    val runDir = Paths.run(context).apply { mkdirs() }
    val frameFile = File(runDir,"frame.bin")
    private var process: GuestProcess? = null
    fun start(id: String, source: String, sound: Boolean) {
        frameFile.delete()
        val request = JSONObject().put("files",context.filesDir.path).put("id",id).put("source",source)
            .put("AF_INPUT_SOCKET",inputSocket).put("AF_APP_PID",android.os.Process.myPid().toString())
        if (sound) request.put("AF_AUDIO_SOCKET",audioSocket)
        val plan = RustBackend.call("prepare",request) as JSONObject
        process = GuestProcess.start(plan)
    }
    fun running() = process?.isAlive == true
    fun exitCode() = process?.let { if (it.isAlive) -1 else it.exitValue() } ?: -1
    fun stop() {
        process?.stop()
        process = null
    }
}

class InputBridge(private val name: String) : Closeable {
    private val queue = LinkedBlockingQueue<String>()
    @Volatile private var running = true
    @Volatile private var socket: LocalSocket? = null
    private val heldKeys = mutableSetOf<String>()
    private val heldButtons = mutableSetOf<Int>()
    private val thread = Thread({
        while (running) {
            try {
                LocalSocket().use { client ->
                    socket = client
                    client.connect(LocalSocketAddress(name,LocalSocketAddress.Namespace.ABSTRACT))
                    val out = client.outputStream
                    while (running) {
                        val first = queue.poll(200,TimeUnit.MILLISECONDS) ?: continue
                        val batch = StringBuilder(first)
                        repeat(128) { queue.poll()?.let { batch.append(it) } }
                        out.write(batch.toString().toByteArray(Charsets.US_ASCII)); out.flush()
                    }
                }
            } catch (_: IOException) { try { Thread.sleep(200) } catch (_: InterruptedException) { break } }
            catch (_: InterruptedException) { break }
            finally { socket = null }
        }
    },"arcade-input").apply { start() }
    fun move(x: Int, y: Int) { if (running) queue.offer("m $x $y\n") }
    @Synchronized fun button(button: Int, down: Boolean) {
        if (down) heldButtons.add(button) else heldButtons.remove(button)
        if (running) queue.offer("b $button ${if(down) 1 else 0}\n")
    }
    @Synchronized fun key(key: String, down: Boolean) {
        if (down) heldKeys.add(key) else heldKeys.remove(key)
        if (running) queue.offer("k $key ${if(down) 1 else 0}\n")
    }
    @Synchronized fun releaseAll() {
        heldKeys.toList().forEach { key(it,false) }; heldButtons.toList().forEach { button(it,false) }
    }
    override fun close() { releaseAll(); running = false; runCatching { socket?.close() }; thread.interrupt() }
}

class AudioBridge(name: String) : Closeable {
    private val server = LocalServerSocket(name)
    @Volatile private var running = true
    @Volatile private var client: LocalSocket? = null
    @Volatile private var track: AudioTrack? = null
    private val thread = Thread({
        while (running) {
            try { server.accept().use { c -> client = c; play(c.inputStream) } }
            catch (_: IOException) { if (!running) break }
            catch (e: Exception) { android.util.Log.w("BananaAudio","Audio failed",e) }
            finally { client = null }
        }
    },"arcade-audio").apply { start() }
    private fun play(raw: InputStream) {
        val input = DataInputStream(raw); val header = ByteArray(12); input.readFully(header)
        check(header.copyOfRange(0,4).toString(Charsets.US_ASCII) == "AFA1") { "Invalid audio header" }
        fun le(o: Int) = (0..3).fold(0) { n,i -> n or ((header[o+i].toInt() and 255) shl (8*i)) }
        val rate = le(4); val channels = le(8)
        check(rate in 8000..192000 && channels in 1..2) { "Invalid audio format" }
        val mask = if (channels==1) AudioFormat.CHANNEL_OUT_MONO else AudioFormat.CHANNEL_OUT_STEREO
        val min = AudioTrack.getMinBufferSize(rate,mask,AudioFormat.ENCODING_PCM_16BIT)
        check(min>0) { "Unsupported audio format" }
        val t = AudioTrack.Builder().setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_GAME).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
            .setAudioFormat(AudioFormat.Builder().setSampleRate(rate).setChannelMask(mask).setEncoding(AudioFormat.ENCODING_PCM_16BIT).build())
            .setBufferSizeInBytes(maxOf(min*2,rate*channels*2/10)).setTransferMode(AudioTrack.MODE_STREAM).build()
        track=t
        try {
            t.play(); val buffer=ByteArray(8192); var carry=0; val frame=channels*2
            while (running) {
                val n=input.read(buffer,carry,buffer.size-carry); if(n<0) break
                val total=n+carry; val usable=total-total%frame
                var offset=0
                while (running && offset<usable) { val written=t.write(buffer,offset,usable-offset); if(written<0) error("Audio output failed"); offset+=written }
                carry=total-usable; if(carry>0) buffer.copyInto(buffer,0,usable,total)
            }
        } finally { track=null; try { t.stop() } catch (_: Exception) {}; t.release() }
    }
    override fun close() { running=false; runCatching { client?.close() }; runCatching { server.close() }; thread.interrupt() }
}

class GameView(context: Context, private val frame: File, private val firstFrame: () -> Unit) : SurfaceView(context), SurfaceHolder.Callback {
    @Volatile var gameRect = RectF(); private set
    @Volatile var frameWidth=640; private set
    @Volatile var frameHeight=480; private set
    @Volatile private var running=false
    private var thread: Thread?=null
    private val lock=Any()
    private var ready=false
    private val paint=Paint(Paint.FILTER_BITMAP_FLAG)
    init { holder.addCallback(this) }
    override fun surfaceCreated(holder: SurfaceHolder) {
        synchronized(lock) { ready=true }; running=true
        thread=Thread({ render() },"arcade-render").apply { start() }
    }
    override fun surfaceChanged(h: SurfaceHolder, format: Int, w: Int, height: Int) { layoutRect(w,height) }
    override fun surfaceDestroyed(holder: SurfaceHolder) {
        synchronized(lock) { ready=false }; running=false; thread?.interrupt(); thread?.join(1000); thread=null
    }
    private fun layoutRect(w: Int,h: Int) {
        val scale=minOf(w.toFloat()/frameWidth,h.toFloat()/frameHeight)
        val width=frameWidth*scale; val height=frameHeight*scale
        gameRect=RectF((w-width)/2,(h-height)/2,(w+width)/2,(h+height)/2)
    }
    private fun render() {
        var header: MappedByteBuffer?=null; var pixels: MappedByteBuffer?=null; var bitmap: Bitmap?=null
        var lastSeq=-1; var announced=false
        try {
            while(running) {
                if(pixels==null) {
                    if(!frame.isFile || frame.length()<32) { Thread.sleep(100); continue }
                    RandomAccessFile(frame,"r").use { raf ->
                        val head=raf.channel.map(FileChannel.MapMode.READ_ONLY,0,32).apply { order(ByteOrder.LITTLE_ENDIAN) }
                        if(head.getInt(0)!=0x42464641) { Thread.sleep(100); return@use }
                        val w=head.getInt(4); val h=head.getInt(8)
                        check(w in 1..4096 && h in 1..4096) { "Invalid frame size" }
                        val size=w.toLong()*h*4
                        if(raf.length()<32+size) return@use
                        frameWidth=w; frameHeight=h; header=head
                        pixels=raf.channel.map(FileChannel.MapMode.READ_ONLY,32,size)
                        bitmap=Bitmap.createBitmap(w,h,Bitmap.Config.ARGB_8888); layoutRect(width,height)
                    }
                    if(pixels==null) { Thread.sleep(100); continue }
                }
                val seq=header!!.getInt(12)
                if(seq==lastSeq) { Thread.sleep(5); continue }; lastSeq=seq
                pixels!!.rewind(); bitmap!!.copyPixelsFromBuffer(pixels!!)
                synchronized(lock) {
                    if(ready && holder.surface.isValid) {
                        val canvas=holder.lockHardwareCanvas()
                        try { canvas.drawColor(Color.BLACK); canvas.drawBitmap(bitmap!!,null,gameRect,paint) }
                        finally { holder.unlockCanvasAndPost(canvas) }
                    }
                }
                if(!announced && hasContent(bitmap!!)) { announced=true; firstFrame() }
            }
        } catch (_: InterruptedException) {} catch(e:Exception) { android.util.Log.e("BananaFrame","Rendering stopped",e) }
        finally { bitmap?.recycle() }
    }
    private fun hasContent(b:Bitmap):Boolean {
        for(y in b.height/8 until b.height step maxOf(1,b.height/8))
            for(x in b.width/16 until b.width step maxOf(1,b.width/16))
                if(b.getPixel(x,y) and 0xffffff != 0) return true
        return false
    }
}
