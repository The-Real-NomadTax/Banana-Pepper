package com.arcadefoundry;

import android.content.Context;

import java.io.File;

/** App-private layout. Everything lives under the app's internal files directory. */
final class Paths {
    private Paths() {}

    static File bin(Context c) { return new File(c.getFilesDir(), "bin"); }
    static File rootfs(Context c) { return new File(c.getFilesDir(), "rootfs"); }
    static File opt(Context c) { return new File(c.getFilesDir(), "opt"); }
    static File tmp(Context c) { return new File(c.getFilesDir(), "tmp"); }
    static File run(Context c) { return new File(c.getFilesDir(), "run"); }
    static File games(Context c) { return new File(c.getFilesDir(), "games"); }
    static File saves(Context c) { return new File(c.getFilesDir(), "saves"); }
    static File prepared(Context c) { return new File(c.getFilesDir(), "prepared"); }

    /** Files pushed with adb (or a file manager) to Android/data/<package>/files/games. */
    static File externalGames(Context c) {
        File ext = c.getExternalFilesDir(null);
        return ext == null ? null : new File(ext, "games");
    }
}
