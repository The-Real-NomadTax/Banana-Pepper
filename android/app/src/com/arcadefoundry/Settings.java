package com.arcadefoundry;

import android.content.Context;
import android.content.SharedPreferences;

/** Per-device preferences shown on the main screen. */
final class Settings {
    private final SharedPreferences prefs;

    Settings(Context context) {
        prefs = context.getSharedPreferences("settings", Context.MODE_PRIVATE);
    }

    boolean sound() {
        return prefs.getBoolean("sound", true);
    }

    void setSound(boolean value) {
        prefs.edit().putBoolean("sound", value).apply();
    }
}
