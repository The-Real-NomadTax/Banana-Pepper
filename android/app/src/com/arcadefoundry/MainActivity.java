package com.arcadefoundry;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Intent;
import android.graphics.Color;
import android.graphics.Typeface;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.util.TypedValue;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.widget.Button;
import android.widget.LinearLayout;
import android.widget.ProgressBar;
import android.widget.ScrollView;
import android.widget.Switch;
import android.widget.TextView;

import java.io.File;

/** Library screen: runtime setup, game import and Play. */
public final class MainActivity extends Activity {
    private static final int PICK_FOLDER = 1;
    private static final int ACCENT = 0xffe8892b;
    private final Handler handler = new Handler(Looper.getMainLooper());

    private TextView runtimeStatus, gameStatus;
    private ProgressBar progress;
    private Button playButton, importButton;
    private volatile boolean busy;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().setStatusBarColor(0xff121212);
        getWindow().setNavigationBarColor(0xff121212);

        ScrollView scroll = new ScrollView(this);
        scroll.setBackgroundColor(0xff121212);
        scroll.setFitsSystemWindows(true);
        LinearLayout page = new LinearLayout(this);
        page.setOrientation(LinearLayout.VERTICAL);
        page.setPadding(dp(20), dp(24), dp(20), dp(24));
        scroll.addView(page);

        TextView title = text("Arcade Foundry", 28, Color.WHITE);
        title.setTypeface(Typeface.DEFAULT_BOLD);
        page.addView(title);
        TextView subtitle = text("Play your own arcade game files on Android", 14, 0xffaaaaaa);
        subtitle.setPadding(0, dp(4), 0, dp(20));
        page.addView(subtitle);

        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(18), dp(18), dp(18), dp(18));
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(0xff1e1e1e);
        bg.setCornerRadius(dp(16));
        card.setBackground(bg);
        page.addView(card, new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT));

        TextView name = text("Big Buck Hunter Pro", 22, Color.WHITE);
        name.setTypeface(Typeface.DEFAULT_BOLD);
        card.addView(name);
        TextView kind = text("Light-gun hunting · 2008 Linux cabinet build", 13, 0xff9a9a9a);
        kind.setPadding(0, dp(2), 0, dp(14));
        card.addView(kind);

        runtimeStatus = text("", 14, 0xffcccccc);
        card.addView(runtimeStatus);
        gameStatus = text("", 14, 0xffcccccc);
        gameStatus.setPadding(0, dp(6), 0, dp(10));
        card.addView(gameStatus);

        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        progress.setMax(100);
        progress.setVisibility(View.GONE);
        card.addView(progress);

        playButton = button("Play", ACCENT, Color.BLACK);
        playButton.setOnClickListener(v -> play());
        card.addView(playButton);
        importButton = button("Import game folder…", 0xff333333, Color.WHITE);
        importButton.setOnClickListener(v -> pickFolder());
        card.addView(importButton);

        Switch sound = new Switch(this);
        sound.setText("Sound");
        sound.setTextColor(0xffdddddd);
        sound.setTextSize(TypedValue.COMPLEX_UNIT_SP, 15);
        sound.setChecked(new Settings(this).sound());
        sound.setOnCheckedChangeListener((b, on) -> new Settings(this).setSound(on));
        sound.setPadding(0, dp(12), 0, 0);
        card.addView(sound);

        TextView help = text("How to play\n"
                + "• Tap the picture to aim and fire.\n"
                + "• Pump (reload): the PUMP button, a second finger, or tap beside the picture.\n"
                + "• COIN / START / TEST are the cabinet buttons. Press TEST at the calibration screen to skip it, "
                + "then START.\n\n"
                + "Game files\n"
                + "Import copies your own \"Big Bug Hunter Pro\" folder (the one containing \"game\" and "
                + "\"bbh3data\", about 640 MB) into the app. Game files are not included with Arcade Foundry.",
                13, 0xff9a9a9a);
        help.setPadding(0, dp(20), 0, 0);
        page.addView(help);

        Button log = button("Show last game log", 0x00000000, 0xff9a9a9a);
        log.setOnClickListener(v -> showLog());
        page.addView(log);

        setContentView(scroll);
        refresh();
        if (!RuntimeInstaller.isInstalled(this)) installRuntime();
    }

    @Override
    protected void onResume() {
        super.onResume();
        if (!busy) refresh();
    }

    private void refresh() {
        boolean runtime = RuntimeInstaller.isInstalled(this);
        File game = BigBuck.findGame(this);
        runtimeStatus.setText(runtime ? "✓ Game runtime installed" : "• Game runtime not installed yet");
        gameStatus.setText(game != null ? "✓ Game files found" : "• Game files needed — tap Import");
        playButton.setEnabled(runtime && game != null && !busy);
        playButton.setAlpha(playButton.isEnabled() ? 1f : 0.4f);
        importButton.setEnabled(!busy);
        importButton.setText(game != null ? "Re-import game folder…" : "Import game folder…");
    }

    private void installRuntime() {
        runTask("Installing game runtime…", () -> RuntimeInstaller.install(this, this::report));
    }

    private void play() {
        File game = BigBuck.findGame(this);
        if (game == null) return;
        runTask("Preparing game…", () -> {
            if (!RuntimeInstaller.isInstalled(this)) RuntimeInstaller.install(this, this::report);
            BigBuck.prepare(this, game, this::report);
        }, () -> startActivity(new Intent(this, GameActivity.class)));
    }

    private void pickFolder() {
        Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT_TREE);
        intent.addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION);
        try {
            startActivityForResult(intent, PICK_FOLDER);
        } catch (Exception e) {
            alert("No folder picker is available on this device.");
        }
    }

    @Override
    protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request != PICK_FOLDER || result != RESULT_OK || data == null || data.getData() == null) return;
        Uri tree = data.getData();
        runTask("Importing…", () -> {
            BigBuck.importTree(this, tree, this::report);
            File game = BigBuck.findGame(this);
            if (game != null) BigBuck.prepare(this, game, this::report);
        });
    }

    // ---- background work -------------------------------------------------------------

    private interface Work {
        void run() throws Exception;
    }

    private void runTask(String message, Work work) {
        runTask(message, work, null);
    }

    private void runTask(String message, Work work, Runnable then) {
        if (busy) return;
        busy = true;
        progress.setVisibility(View.VISIBLE);
        progress.setProgress(0);
        gameStatus.setText(message);
        refresh();
        new Thread(() -> {
            String error = null;
            try {
                work.run();
            } catch (Exception e) {
                error = e.getMessage() != null ? e.getMessage() : e.toString();
            }
            String failure = error;
            handler.post(() -> {
                busy = false;
                progress.setVisibility(View.GONE);
                refresh();
                if (failure != null) alert(failure);
                else if (then != null) then.run();
            });
        }, "arcade-task").start();
    }

    private void report(String message, int percent) {
        handler.post(() -> {
            runtimeStatus.setText(message);
            progress.setProgress(percent);
        });
    }

    private void showLog() {
        File run = Paths.run(this);
        String text = GameActivity.tail(new File(run, "session.log"), 30) + "\n"
                + GameActivity.tail(new File(run, "game.log"), 40);
        alert(text.trim().isEmpty() ? "No game has been started yet." : text);
    }

    // ---- widgets ---------------------------------------------------------------------

    private void alert(String message) {
        new AlertDialog.Builder(this).setMessage(message).setPositiveButton("OK", null).show();
    }

    private TextView text(String s, int sp, int color) {
        TextView t = new TextView(this);
        t.setText(s);
        t.setTextSize(TypedValue.COMPLEX_UNIT_SP, sp);
        t.setTextColor(color);
        return t;
    }

    private Button button(String label, int color, int textColor) {
        Button b = new Button(this);
        b.setText(label);
        b.setAllCaps(false);
        b.setTextSize(TypedValue.COMPLEX_UNIT_SP, 16);
        b.setTextColor(textColor);
        b.setGravity(Gravity.CENTER);
        GradientDrawable bg = new GradientDrawable();
        bg.setColor(color);
        bg.setCornerRadius(dp(12));
        b.setBackground(bg);
        b.setStateListAnimator(null);
        LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(ViewGroup.LayoutParams.MATCH_PARENT, dp(52));
        lp.setMargins(0, dp(10), 0, 0);
        b.setLayoutParams(lp);
        return b;
    }

    private int dp(int v) {
        return Math.round(v * getResources().getDisplayMetrics().density);
    }
}
