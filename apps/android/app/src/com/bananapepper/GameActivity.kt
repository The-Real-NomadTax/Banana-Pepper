package com.bananapepper

import android.app.*
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.*
import android.view.*
import android.widget.*
import org.json.JSONObject
import java.io.File
import java.util.concurrent.Executors

class GameActivity : Activity() {
    private val handler=Handler(Looper.getMainLooper())
    private val worker=Executors.newSingleThreadExecutor()
    private lateinit var session:GameSession
    private lateinit var picture:GameView
    private lateinit var loading:TextView
    private var input:InputBridge?=null
    private var audio:AudioBridge?=null
    private var aim=-1
    private var secondary=-1
    private var failed=false
    @Volatile private var closing=false
    private val watch=object:Runnable {
        override fun run() {
            if(closing || isFinishing || failed)return
            if(!session.running()) {
                if(session.exitCode()==0)finish() else showError("The game stopped (${session.exitCode()}).\n\n"+tail(File(session.runDir,"game.log"),18)+"\n"+tail(File(session.runDir,"session.log"),12))
            } else handler.postDelayed(this,1000)
        }
    }
    override fun onCreate(state:Bundle?) {
        super.onCreate(state)
        val game=try { JSONObject(intent.getStringExtra("game")?:error("Choose a game from the library")) } catch(_:Exception) { finish(); return }
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        window.attributes=window.attributes.apply { layoutInDisplayCutoutMode=WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES }
        session=GameSession(this)
        val root=FrameLayout(this).apply { setBackgroundColor(Color.BLACK) }
        picture=GameView(this,session.frameFile) { handler.post { if(!closing)loading.visibility=View.GONE } }
        root.addView(picture,FrameLayout.LayoutParams(-1,-1).apply {
            marginStart=dp(124); marginEnd=dp(124)
        })
        picture.setOnTouchListener { _,event -> touch(event) }
        val left=column(); val right=column()
        val controls=game.getJSONArray("controls")
        // Cabinet switch labels and keys come from the Rust game declaration.
        for(i in 0 until controls.length()) {
            val action=controls.getJSONObject(i)
            val key=action.getString("key")
            val b=keyButton(action.getString("label"),key)
            if(i%2==0)left.addView(b) else right.addView(b)
        }
        if(game.getString("kind")=="LightGuns")right.addView(button("RELOAD").apply {
            setOnTouchListener { _,e -> when(e.actionMasked) { MotionEvent.ACTION_DOWN->input?.button(3,true); MotionEvent.ACTION_UP,MotionEvent.ACTION_CANCEL->input?.button(3,false) }; true }
        })
        right.addView(button("EXIT").apply { setOnClickListener { confirmExit() } })
        // Scrollable side controls remain reachable for every registered cabinet.
        root.addView(ScrollView(this).apply { addView(left); isVerticalScrollBarEnabled=false },FrameLayout.LayoutParams(dp(124),-1,Gravity.START))
        root.addView(ScrollView(this).apply { addView(right); isVerticalScrollBarEnabled=false },FrameLayout.LayoutParams(dp(124),-1,Gravity.END))
        loading=TextView(this).apply { text="Starting ${game.getString("title")}…"; textSize=16f; setTextColor(Color.WHITE); gravity=Gravity.CENTER; setPadding(dp(140),0,dp(140),0) }
        root.addView(loading,FrameLayout.LayoutParams(-1,-2,Gravity.CENTER))
        setContentView(root)
        window.decorView.systemUiVisibility=View.SYSTEM_UI_FLAG_FULLSCREEN or View.SYSTEM_UI_FLAG_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or View.SYSTEM_UI_FLAG_LAYOUT_STABLE
        worker.execute {
            try {
                input=InputBridge(session.inputSocket)
                val sound=getSharedPreferences("settings",MODE_PRIVATE).getBoolean("sound",true)
                if(sound)audio=AudioBridge(session.audioSocket)
                session.start(game.getString("id"),game.getString("source"),sound)
                if(closing)session.stop() else handler.postDelayed(watch,1000)
            } catch(e:Exception) { handler.post { if(!closing)showError(e.message?:e.toString()) } }
        }
    }
    private fun touch(e:MotionEvent):Boolean {
        val bridge=input?:return true
        val id=e.getPointerId(e.actionIndex)
        when(e.actionMasked) {
            MotionEvent.ACTION_DOWN,MotionEvent.ACTION_POINTER_DOWN -> {
                if(aim<0 && picture.gameRect.contains(e.getX(e.actionIndex),e.getY(e.actionIndex))) {
                    aim=id; aim(e.getX(e.actionIndex),e.getY(e.actionIndex)); bridge.button(1,true)
                } else if(secondary<0) { secondary=id; bridge.button(3,true) }
            }
            MotionEvent.ACTION_MOVE -> { val index=e.findPointerIndex(aim); if(index>=0)aim(e.getX(index),e.getY(index)) }
            MotionEvent.ACTION_POINTER_UP -> {
                if(id==aim) { bridge.button(1,false); aim=-1 }
                if(id==secondary) { bridge.button(3,false); secondary=-1 }
            }
            MotionEvent.ACTION_UP,MotionEvent.ACTION_CANCEL -> release()
        }
        return true
    }
    private fun aim(x:Float,y:Float) {
        val r=picture.gameRect; if(r.width()<=0 || r.height()<=0)return
        input?.move(((x-r.left)*picture.frameWidth/r.width()).toInt().coerceIn(0,picture.frameWidth-1),((y-r.top)*picture.frameHeight/r.height()).toInt().coerceIn(0,picture.frameHeight-1))
    }
    private fun release() { input?.releaseAll(); aim=-1; secondary=-1 }
    private fun keyButton(label:String,key:String)=button(label).apply {
        setOnTouchListener { v,e ->
            when(e.actionMasked) {
                MotionEvent.ACTION_DOWN -> { v.isPressed=true; input?.key(key,true) }
                MotionEvent.ACTION_UP,MotionEvent.ACTION_CANCEL -> { v.isPressed=false; input?.key(key,false) }
            }; true
        }
    }
    private fun column()=LinearLayout(this).apply { orientation=LinearLayout.VERTICAL; setPadding(dp(8),dp(12),dp(8),dp(12)) }
    private fun button(label:String)=Button(this).apply {
        text=label; textSize=11f; isAllCaps=false; setTextColor(Color.WHITE)
        background=GradientDrawable().apply { setColor(0xb32b382a.toInt()); cornerRadius=dp(12).toFloat(); setStroke(dp(1),0x44d9f26d) }
        layoutParams=LinearLayout.LayoutParams(-1,dp(48)).apply { topMargin=dp(6); bottomMargin=dp(6) }
    }
    private fun dp(v:Int)=(v*resources.displayMetrics.density).toInt()
    private fun confirmExit() { release(); AlertDialog.Builder(this).setMessage("Close this game?").setPositiveButton("Close") { _,_->finish() }.setNegativeButton("Keep playing",null).show() }
    @Deprecated("Platform back callback") override fun onBackPressed() { confirmExit() }
    private fun showError(message:String) {
        if(failed || isFinishing || isDestroyed)return
        failed=true; loading.visibility=View.GONE; release()
        AlertDialog.Builder(this).setTitle("Game session").setMessage(message).setCancelable(false).setPositiveButton("Back to library") { _,_->finish() }.show()
    }
    override fun onPause() { release(); super.onPause() }
    override fun onDestroy() {
        closing=true; handler.removeCallbacksAndMessages(null)
        if(::session.isInitialized)worker.execute {
            session.stop(); input?.close(); audio?.close()
        }
        worker.shutdown()
        super.onDestroy()
    }
}
