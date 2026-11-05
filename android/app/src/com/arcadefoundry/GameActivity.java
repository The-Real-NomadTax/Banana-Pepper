package com.arcadefoundry;

import android.app.Activity;
import android.app.AlertDialog;
import android.graphics.Color;
import android.graphics.RectF;
import android.graphics.drawable.GradientDrawable;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.View;
import android.view.ViewGroup;
import android.view.WindowInsets;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.widget.Button;
import android.widget.FrameLayout;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.TextView;

import java.io.File;
import java.io.IOException;
import java.io.RandomAccessFile;

/**
 * Fullscreen game screen. Touch the picture to aim and shoot; side buttons
 * give the cabinet's coin/start/test switches and the shotgun pump.
 */
public final class GameActivity extends Activity implements GameView.Listener {
    private GameSession session;
    private InputBridge input;
    private AudioBridge audio;
    private GameView view;
    private View loading;
    private TextView loadingText;
    private final Handler handler = new Handler(Looper.getMainLooper());
    private int aimPointer = -1;
    private int pumpPointer = -1;
    private boolean finishedNormally;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        getWindow().getAttributes().layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_SHORT_EDGES;

        File gameDir = BigBuck.findGame(this);
        if (gameDir == null || !RuntimeInstaller.isInstalled(this)) {
            finish();
            return;
        }
        session = new GameSession(this);
        Settings settings = new Settings(this);

        FrameLayout root = new FrameLayout(this);
        root.setBackgroundColor(Color.BLACK);
        view = new GameView(this, session.frameFile, this);
        root.addView(view, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT));
        view.setOnTouchListener((v, e) -> onGameTouch(e));

        LinearLayout left = column();
        left.addView(switchButton("COIN", "5"));
        left.addView(switchButton("START", "1"));
        left.addView(switchButton("P2 START", "2"));
        root.addView(left, sideParams(Gravity.START | Gravity.CENTER_VERTICAL));

        LinearLayout right = column();
        right.addView(pumpButton());
        right.addView(switchButton("TEST", "t"));
        Button exit = styled("EXIT", 0x66b00020);
        exit.setOnClickListener(v -> confirmExit());
        right.addView(exit);
        root.addView(right, sideParams(Gravity.END | Gravity.CENTER_VERTICAL));

        LinearLayout box = new LinearLayout(this);
        box.setOrientation(LinearLayout.VERTICAL);
        box.setGravity(Gravity.CENTER);
        ProgressBar spinner = new ProgressBar(this);
        box.addView(spinner);
        loadingText = new TextView(this);
        loadingText.setText("Starting Big Buck Hunter Pro…");
        loadingText.setTextColor(Color.WHITE);
        loadingText.setTextSize(TypedValue.COMPLEX_UNIT_SP, 18);
        loadingText.setGravity(Gravity.CENTER);
        loadingText.setPadding(0, dp(16), 0, 0);
        box.addView(loadingText);
        loading = box;
        root.addView(box, new FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT, Gravity.CENTER));

        setContentView(root);
        hideSystemBars();

        try {
            File prepared = BigBuck.preparedExecutable(this);
            File saves = BigBuck.saves(this, gameDir);
            input = new InputBridge(session.inputSocket);
            if (settings.sound()) audio = new AudioBridge(session.audioSocket);
            session.start(this, gameDir, prepared, saves, settings.sound());
        } catch (IOException e) {
            showError("Couldn't start the game: " + e.getMessage());
            return;
        }
        handler.postDelayed(this::watch, 1000);
    }

    private void watch() {
        if (isFinishing() || session == null) return;
        if (!session.isRunning()) {
            if (finishedNormally) return;
            int code = session.exitCode();
            if (code == 0) {
                finish();
            } else {
                showError("The game stopped (exit code " + code + ").\n\n" + tail(session.logFile, 12)
                        + tail(new File(session.runDir, "game.log"), 12));
            }
            return;
        }
        handler.postDelayed(this::watch, 1000);
    }

    @Override
    public void onFirstFrame() {
        handler.post(() -> loading.setVisibility(View.GONE));
    }

    // ---- touch -----------------------------------------------------------------------

    private boolean onGameTouch(MotionEvent e) {
        if (input == null) return true;
        int action = e.getActionMasked();
        int index = e.getActionIndex();
        int id = e.getPointerId(index);
        switch (action) {
            case MotionEvent.ACTION_DOWN:
            case MotionEvent.ACTION_POINTER_DOWN: {
                float x = e.getX(index), y = e.getY(index);
                RectF r = view.gameRect();
                boolean inside = r.contains(x, y);
                if (aimPointer < 0 && inside) {
                    aimPointer = id;
                    sendAim(x, y);
                    input.button(1, true); // trigger
                } else if (pumpPointer < 0) {
                    // Second finger anywhere, or a tap beside the picture: pump / reload.
                    pumpPointer = id;
                    input.button(3, true);
                }
                return true;
            }
            case MotionEvent.ACTION_MOVE:
                if (aimPointer >= 0) {
                    int i = e.findPointerIndex(aimPointer);
                    if (i >= 0) sendAim(e.getX(i), e.getY(i));
                }
                return true;
            case MotionEvent.ACTION_UP:
            case MotionEvent.ACTION_POINTER_UP:
            case MotionEvent.ACTION_CANCEL:
                if (id == aimPointer || action == MotionEvent.ACTION_CANCEL) {
                    if (aimPointer >= 0) input.button(1, false);
                    aimPointer = -1;
                }
                if (id == pumpPointer || action == MotionEvent.ACTION_CANCEL) {
                    if (pumpPointer >= 0) input.button(3, false);
                    pumpPointer = -1;
                }
                if (action == MotionEvent.ACTION_UP) {
                    if (aimPointer >= 0) input.button(1, false);
                    if (pumpPointer >= 0) input.button(3, false);
                    aimPointer = pumpPointer = -1;
                }
                return true;
            default:
                return true;
        }
    }

    private void sendAim(float x, float y) {
        RectF r = view.gameRect();
        int gx = (int) ((x - r.left) * view.frameWidth() / r.width());
        int gy = (int) ((y - r.top) * view.frameHeight() / r.height());
        gx = Math.max(0, Math.min(view.frameWidth() - 1, gx));
        gy = Math.max(0, Math.min(view.frameHeight() - 1, gy));
        input.move(gx, gy);
    }

    // ---- buttons ---------------------------------------------------------------------

    private LinearLayout column() {
        LinearLayout l = new LinearLayout(this);
        l.setOrientation(LinearLayout.VERTICAL);
        l.setGravity(Gravity.CENTER);
        l.setPadding(dp(12), dp(12), dp(12), dp(12));
        return l;
    }

    private FrameLayout.LayoutParams sideParams(int gravity) {
        return new FrameLayout.LayoutParams(ViewGroup.LayoutParams.WRAP_CONTENT,
                ViewGroup.LayoutParams.WRAP_CONTENT, gravity);
    }

    private Button styled(String label, int color) {
        Button b = new Button(this);
        b.setText(label);
        b.setTextColor(Color.WHITE);
        b.setTextSize(TypedValue.COMPLEX_UNIT_SP, 14);
        b.setAllCaps(true);
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(color);
        bg.setCornerRadius(dp(14));
        bg.setStroke(dp(1), 0x55ffffff);
        b.setBackground(bg);
        b.setStateListAnimator(null);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(dp(104), dp(56));
        lp.setMargins(0, dp(8), 0, dp(8));
        b.setLayoutParams(lp);
        return b;
    }

    private Button switchButton(String label, String keysym) {
        Button b = styled(label, 0x66303030);
        b.setOnTouchListener((v, e) -> {
            if (input == null) return false;
            if (e.getActionMasked() == MotionEvent.ACTION_DOWN) {
                v.setPressed(true);
                input.key(keysym, true);
            } else if (e.getActionMasked() == MotionEvent.ACTION_UP
                    || e.getActionMasked() == MotionEvent.ACTION_CANCEL) {
                v.setPressed(false);
                input.key(keysym, false);
            }
            return true;
        });
        return b;
    }

    private Button pumpButton() {
        Button b = styled("PUMP", 0x66206030);
        b.setOnTouchListener((v, e) -> {
            if (input == null) return false;
            if (e.getActionMasked() == MotionEvent.ACTION_DOWN) input.button(3, true);
            else if (e.getActionMasked() == MotionEvent.ACTION_UP
                    || e.getActionMasked() == MotionEvent.ACTION_CANCEL) input.button(3, false);
            return true;
        });
        return b;
    }

    // ---- lifecycle -------------------------------------------------------------------

    @Override
    public void onBackPressed() {
        confirmExit();
    }

    private void confirmExit() {
        new AlertDialog.Builder(this)
                .setTitle("Quit Big Buck Hunter?")
                .setPositiveButton("Quit", (d, w) -> finish())
                .setNegativeButton("Keep playing", null)
                .show();
    }

    @Override
    public void onWindowFocusChanged(boolean hasFocus) {
        super.onWindowFocusChanged(hasFocus);
        if (hasFocus) hideSystemBars();
    }

    @Override
    protected void onPause() {
        super.onPause();
        if (audio != null) audio.setPaused(true);
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (audio != null) audio.setPaused(false);
    }

    @Override
    protected void onDestroy() {
        finishedNormally = true;
        handler.removeCallbacksAndMessages(null);
        if (session != null) session.stop();
        if (input != null) input.close();
        if (audio != null) audio.close();
        super.onDestroy();
    }

    @SuppressWarnings("deprecation")
    private void hideSystemBars() {
        if (android.os.Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController c = getWindow().getInsetsController();
            if (c != null) {
                c.hide(WindowInsets.Type.systemBars());
                c.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            }
        } else {
            getWindow().getDecorView().setSystemUiVisibility(View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    | View.SYSTEM_UI_FLAG_FULLSCREEN | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_LAYOUT_STABLE | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION);
        }
    }

    private void showError(String message) {
        if (isFinishing()) return;
        loading.setVisibility(View.GONE);
        new AlertDialog.Builder(this)
                .setTitle("Big Buck Hunter Pro")
                .setMessage(message)
                .setCancelable(false)
                .setPositiveButton("Back", (d, w) -> finish())
                .show();
    }

    static String tail(File f, int lines) {
        try (RandomAccessFile raf = new RandomAccessFile(f, "r")) {
            long len = raf.length();
            long start = Math.max(0, len - 4096);
            byte[] b = new byte[(int) (len - start)];
            raf.seek(start);
            raf.readFully(b);
            String[] all = new String(b).split("\n");
            StringBuilder sb = new StringBuilder();
            for (int i = Math.max(0, all.length - lines); i < all.length; i++) {
                if (all[i].startsWith("RSP") || all[i].startsWith("R") && all[i].contains(":0x")) continue;
                sb.append(all[i]).append('\n');
            }
            return sb.toString();
        } catch (IOException e) {
            return "";
        }
    }

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }
}
