package com.bananapepper

import android.app.*
import android.content.*
import android.graphics.Color
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.net.Uri
import android.os.*
import android.provider.DocumentsContract
import android.provider.OpenableColumns
import android.view.*
import android.widget.*
import org.json.*
import java.io.File
import java.util.concurrent.Executors

/** Native Kotlin library backed by the Rust game registry. */
class MainActivity : Activity() {
    private val worker=Executors.newSingleThreadExecutor()
    private val handler=Handler(Looper.getMainLooper())
    private lateinit var cards:LinearLayout
    private lateinit var status:TextView
    private lateinit var progress:ProgressBar
    private lateinit var importButton:Button
    private var entries=JSONArray()
    private var busy=false
    private var pendingImport:String?=null
    private var selectedFilter="all"
    private val ink=0xfff2f3e8.toInt()
    private val muted=0xffa1a99f.toInt()
    private val accent=0xffd9f26d.toInt()
    override fun onCreate(state:Bundle?) {
        super.onCreate(state)
        pendingImport=state?.getString("import")
        window.statusBarColor=0xff101813.toInt(); window.navigationBarColor=0xff101813.toInt()
        val scroll=ScrollView(this).apply { setBackgroundColor(0xff101813.toInt()); isFillViewport=true; fitsSystemWindows=true }
        val page=LinearLayout(this).apply { orientation=LinearLayout.VERTICAL; setPadding(dp(22),dp(24),dp(22),dp(30)) }
        scroll.addView(page)
        page.addView(text("BANANA PEPPER",12,accent).apply { letterSpacing=.18f; setTypeface(null,Typeface.BOLD) })
        page.addView(text("Your arcade.\nAnywhere.",36,ink).apply { setTypeface(null,Typeface.BOLD); setPadding(0,dp(12),0,dp(8)) })
        page.addView(text("Your games, brought to Android.",15,muted))
        val panel=LinearLayout(this).apply { orientation=LinearLayout.VERTICAL; background=background(0xff1d2920.toInt(),18); setPadding(dp(18),dp(16),dp(18),dp(16)) }
        page.addView(panel,LinearLayout.LayoutParams(-1,-2).apply { topMargin=dp(24); bottomMargin=dp(24) })
        status=text("Checking your library…",14,ink); panel.addView(status)
        progress=ProgressBar(this,null,android.R.attr.progressBarStyleHorizontal).apply { max=100; visibility=View.GONE }
        panel.addView(progress)
        importButton=button("Import a game",accent,0xff16220d.toInt()).apply { setOnClickListener { chooseImport() } }
        panel.addView(importButton)
        val filters=LinearLayout(this)
        for ((key,label) in listOf("all" to "All games","ready" to "Ready to play")) {
            filters.addView(button(label,0xff253229.toInt(),ink).apply { setOnClickListener { selectedFilter=key; renderCards() } },LinearLayout.LayoutParams(0,dp(48),1f).apply { marginEnd=dp(8) })
        }
        page.addView(filters)
        cards=LinearLayout(this).apply { orientation=LinearLayout.VERTICAL }; page.addView(cards)
        val sound=Switch(this).apply {
            text="Game sound"; setTextColor(ink); textSize=16f
            isChecked=getSharedPreferences("settings",MODE_PRIVATE).getBoolean("sound",true)
            setPadding(0,dp(24),0,dp(12))
            setOnCheckedChangeListener { _,checked -> getSharedPreferences("settings",MODE_PRIVATE).edit().putBoolean("sound",checked).apply() }
        }
        page.addView(sound)
        page.addView(button("Session logs",0xff253229.toInt(),ink).apply { setOnClickListener { showLog() } })
        page.addView(text("Game files are supplied by you. Android compatibility is shown separately for each title.",12,muted).apply { setPadding(0,dp(20),0,0) })
        setContentView(scroll)
        if(!RuntimeInstaller.installed(this)) task("Installing runtime…", {
            RuntimeInstaller.install(this) { message,pct -> handler.post { status.text=message; progress.isIndeterminate=pct<0; if(pct>=0)progress.progress=pct } }
        }) else refresh()
    }
    override fun onResume() { super.onResume(); if(::cards.isInitialized && !busy)refresh() }
    override fun onSaveInstanceState(out:Bundle) { super.onSaveInstanceState(out); out.putString("import",pendingImport) }
    private fun refresh() {
        if(busy)return
        busy=true
        worker.execute {
            try {
                val result=RustBackend.catalog(this)
                handler.post { if(!isDestroyed) { busy=false; entries=result; renderCards(); status.text="${readyCount()} ready · ${entries.length()} in the collection"; importButton.isEnabled=true } }
            } catch(e:Throwable) { handler.post { busy=false; if(!isDestroyed) { status.text="Library unavailable"; alert(e.message?:e.toString()) } } }
        }
    }
    private fun readyCount()=(0 until entries.length()).count { entries.getJSONObject(it).getBoolean("ready") }
    private fun renderCards() {
        cards.removeAllViews()
        val visible=(0 until entries.length()).map { entries.getJSONObject(it) }.filter { selectedFilter=="all" || it.getBoolean("ready") }
        if(visible.isEmpty())cards.addView(text("No games ready yet. Import your files to get started.",15,muted).apply { setPadding(0,dp(24),0,dp(24)) })
        for(game in visible) {
            val ready=game.getBoolean("ready"); val found=game.getBoolean("found"); val missing=game.getJSONArray("missing")
            val card=LinearLayout(this).apply { orientation=LinearLayout.VERTICAL; setPadding(dp(18),dp(20),dp(18),dp(18)); background=background(0xff1a241d.toInt(),20) }
            cards.addView(card,LinearLayout.LayoutParams(-1,-2).apply { topMargin=dp(14) })
            val kind=when(game.getString("kind")) { "LightGuns"->"LIGHT GUN"; "Driving"->"RACING"; else->"ARCADE" }
            card.addView(text(kind,10,accent).apply { letterSpacing=.16f; setTypeface(null,Typeface.BOLD) })
            card.addView(text(game.getString("title"),22,ink).apply { setTypeface(null,Typeface.BOLD); setPadding(0,dp(9),0,dp(8)) })
            card.addView(text(game.getString("status"),12,if(game.getBoolean("verified"))accent else muted))
            val detail=if(!found) "Add your game files" else if(missing.length()>0) "Runtime components needed" else "Files found · ready to launch"
            card.addView(text(detail,14,ink).apply { setPadding(0,dp(14),0,dp(4)) })
            val row=LinearLayout(this)
            row.addView(button(if(ready)"Play  ›" else "Details",if(ready)accent else 0xff2a382d.toInt(),if(ready)0xff18240e.toInt() else ink).apply {
                setOnClickListener { if(ready)play(game) else details(game) }
            },LinearLayout.LayoutParams(0,dp(48),1f).apply { topMargin=dp(10); marginEnd=dp(8) })
            row.addView(button("Import",0xff2a382d.toInt(),ink).apply { setOnClickListener { pickSource(game.getString("id")) } },LinearLayout.LayoutParams(dp(94),dp(48)).apply { topMargin=dp(10) })
            card.addView(row)
        }
    }
    private fun details(game:JSONObject) {
        val missing=game.getJSONArray("missing")
        val message=buildString {
            append(game.getString("status")); append("\n\n")
            if(!game.getBoolean("found"))append("Import the folder containing ${game.getString("executable")}, or a .zar archive of it.\n\n")
            if(missing.length()>0)append("Missing runtime components:\n"+(0 until missing.length()).joinToString("\n") { missing.getString(it) }+"\n\n")
        }.trimEnd()
        AlertDialog.Builder(this).setTitle(game.getString("title")).setMessage(message).setPositiveButton("OK",null).show()
    }
    private fun chooseImport() {
        val games=(0 until entries.length()).map { entries.getJSONObject(it) }
        AlertDialog.Builder(this).setTitle("Which game are you importing?").setItems(games.map { it.getString("title") }.toTypedArray()) { _,which -> pickSource(games[which].getString("id")) }.show()
    }
    private fun pickSource(id:String) {
        if(busy)return
        AlertDialog.Builder(this).setTitle("Import from").setItems(arrayOf("Game folder","ZAR archive (.zar)")) { _,which ->
            if(which==0)pickFolder(id) else pickArchive(id)
        }.show()
    }
    private fun pickFolder(id:String) {
        if(busy)return
        pendingImport=id
        startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT_TREE).addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),1)
    }
    /** ZAR has no registered MIME type, so any openable file is offered and the name is checked. */
    private fun pickArchive(id:String) {
        if(busy)return
        pendingImport=id
        startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType("*/*")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION),2)
    }
    @Deprecated("Platform activity result for SDK-only build")
    override fun onActivityResult(request:Int,result:Int,data:Intent?) {
        super.onActivityResult(request,result,data)
        val tree=data?.data ?: return; val id=pendingImport ?: return
        if(result!=RESULT_OK)return
        if(request==2) { importArchive(tree,id); return }
        if(request!=1)return
        task("Importing your files…", {
            val parent=File(filesDir,"games").apply { mkdirs() }
            val staging=File(parent,"$id.importing")
            RuntimeInstaller.deleteTree(staging); staging.mkdirs()
            try {
                importTree(tree,DocumentsContract.getTreeDocumentId(tree),staging,0)
                val inspected=RustBackend.call("catalog",JSONObject().put("files",filesDir.path).put("roots",JSONArray().put(staging.path))) as JSONArray
                val identified=(0 until inspected.length()).map { inspected.getJSONObject(it) }.first { it.getString("id")==id }
                check(identified.getBoolean("found")) { "This folder does not contain a recognized build of the selected game. Your previous import was kept." }
                val relative=File(identified.getString("source")).relativeTo(staging)
                val dest=File(parent,id); val previous=File(parent,"$id.previous")
                RuntimeInstaller.deleteTree(previous)
                if(dest.exists())check(dest.renameTo(previous)) { "Cannot keep previous import" }
                if(!staging.renameTo(dest)) { previous.renameTo(dest); error("Cannot activate import") }
                RuntimeInstaller.deleteTree(previous)
                File(parent,"$id.zar").delete()
                RustBackend.saveSource(this,id,File(dest,relative.path).path)
            } catch(e:Exception) { RuntimeInstaller.deleteTree(staging); throw e }
        })
    }
    /** Copy one archive into private storage. It is staged outside the scanned
     *  library, so a partial copy is never offered as a game, and it replaces the
     *  game's previous folder or archive only once it is recognized. */
    private fun importArchive(uri:Uri,id:String) {
        var name=""; var size=-1L
        contentResolver.query(uri,arrayOf(OpenableColumns.DISPLAY_NAME,OpenableColumns.SIZE),null,null,null)?.use { c ->
            if(c.moveToFirst()) { name=c.getString(0)?:""; if(!c.isNull(1))size=c.getLong(1) }
        }
        val extension=name.substringAfterLast('.',"").lowercase()
        if(extension !in setOf("zar","zarchive","wua")) { alert("Choose a .zar archive. \"$name\" is not one."); return }
        task("Importing $name…", {
            val staging=File(filesDir,"import").apply { RuntimeInstaller.deleteTree(this); mkdirs() }
            val parent=File(filesDir,"games").apply { mkdirs() }
            if(size>0)check(staging.usableSpace>size+(256L shl 20)) { "Not enough free space to import $name (${size shr 20} MB)." }
            val staged=File(staging,"$id.$extension")
            try {
                contentResolver.openInputStream(uri).use { input ->
                    check(input!=null) { "Cannot read $name" }
                    staged.outputStream().use { output ->
                        val buffer=ByteArray(1 shl 20); var copied=0L; var shown=-1L
                        while(true) {
                            val n=input.read(buffer); if(n<0)break
                            output.write(buffer,0,n); copied+=n
                            if(copied shr 23!=shown) {
                                shown=copied shr 23
                                val pct=if(size>0)(copied*100/size).toInt() else -1; val mb=copied shr 20
                                handler.post { progress.isIndeterminate=pct<0; if(pct>=0)progress.progress=pct; status.text="Importing $name… $mb MB" }
                            }
                        }
                    }
                }
                val inspected=RustBackend.call("catalog",JSONObject().put("files",filesDir.path).put("roots",JSONArray().put(staged.path))) as JSONArray
                val identified=(0 until inspected.length()).map { inspected.getJSONObject(it) }.first { it.getString("id")==id }
                check(identified.getBoolean("found")) { "$name does not contain a recognized build of the selected game. Your previous import was kept." }
                val dest=File(parent,"$id.$extension")
                check(staged.renameTo(dest)) { "Cannot activate import" }
                for(old in listOf("zar","zarchive","wua"))if(old!=extension)File(parent,"$id.$old").delete()
                RuntimeInstaller.deleteTree(File(parent,id))
                RustBackend.saveSource(this,id,dest.path)
            } finally { RuntimeInstaller.deleteTree(staging) }
        })
    }
    private fun importTree(tree:Uri,id:String,dest:File,depth:Int) {
        check(depth<64) { "The selected folder is too deeply nested" }
        val uri=DocumentsContract.buildChildDocumentsUriUsingTree(tree,id)
        contentResolver.query(uri,arrayOf("document_id","_display_name","mime_type"),null,null,null).use { cursor ->
            check(cursor!=null) { "Cannot read selected folder" }
            while(cursor.moveToNext()) {
                val child=cursor.getString(0); val name=cursor.getString(1); val mime=cursor.getString(2)
                check(name.isNotEmpty() && name!="." && name!=".." && '/' !in name && '\u0000' !in name) { "Invalid file name" }
                val target=File(dest,name)
                if(mime==DocumentsContract.Document.MIME_TYPE_DIR) { target.mkdirs(); importTree(tree,child,target,depth+1) }
                else {
                    contentResolver.openInputStream(DocumentsContract.buildDocumentUriUsingTree(tree,child)).use { input ->
                        check(input!=null) { "Cannot read $name" }
                        target.outputStream().use { input.copyTo(it,131072) }
                    }
                }
            }
        }
    }
    private fun play(game:JSONObject) {
        if(busy)return
        startActivity(Intent(this,GameActivity::class.java).putExtra("game",game.toString()))
    }
    private fun task(message:String,work:()->Unit) {
        if(busy)return
        busy=true; importButton.isEnabled=false; progress.visibility=View.VISIBLE; progress.isIndeterminate=true; status.text=message
        worker.execute {
            var failure:String?=null
            try { work() } catch(e:Exception) { android.util.Log.e("BananaRuntime",message,e); failure=e.message?:e.toString() }
            handler.post { if(!isDestroyed) { busy=false; progress.visibility=View.GONE; importButton.isEnabled=true; failure?.let { alert(it) }; refresh() } }
        }
    }
    private fun showLog() {
        val run=getSharedPreferences("runtime",MODE_PRIVATE).getString("last_run",null)?.let { File(it) } ?: Paths.run(this)
        alert(listOf("session.log","game.log","xvfb.log").joinToString("\n\n") { "$it\n${tail(File(run,it),35)}" })
    }
    private fun alert(message:String) { if(!isFinishing && !isDestroyed)AlertDialog.Builder(this).setMessage(message).setPositiveButton("OK",null).show() }
    override fun onDestroy() { handler.removeCallbacksAndMessages(null); worker.shutdown(); super.onDestroy() }
    private fun dp(v:Int)=(v*resources.displayMetrics.density).toInt()
    private fun text(value:String,size:Int,color:Int)=TextView(this).apply { text=value; textSize=size.toFloat(); setTextColor(color) }
    private fun background(color:Int,radius:Int)=GradientDrawable().apply { setColor(color); cornerRadius=dp(radius).toFloat() }
    private fun button(label:String,color:Int,textColor:Int)=Button(this).apply {
        text=label; isAllCaps=false; textSize=14f; setTextColor(textColor); background=background(color,12); stateListAnimator=null
        layoutParams=LinearLayout.LayoutParams(-1,dp(48)).apply { topMargin=dp(12) }
    }
}

fun tail(file:File,lines:Int):String = try {
    java.io.RandomAccessFile(file,"r").use { f ->
        f.seek(maxOf(0,f.length()-24576)); val data=ByteArray((f.length()-f.filePointer).toInt()); f.readFully(data)
        data.toString(Charsets.UTF_8).lines().takeLast(lines).joinToString("\n")
    }
} catch(_:Exception) { "No log yet" }
