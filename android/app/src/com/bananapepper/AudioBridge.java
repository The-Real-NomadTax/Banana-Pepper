package com.bananapepper;

import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioTrack;
import android.net.LocalServerSocket;
import android.net.LocalSocket;
import android.util.Log;

import java.io.DataInputStream;
import java.io.IOException;
import java.io.InputStream;

/**
 * Receives the game's PCM (S16LE) from the guest shim over an abstract socket
 * and plays it with AudioTrack. Blocking writes pace the game's audio thread.
 */
final class AudioBridge implements Runnable {
    private static final String TAG = "ArcadeAudio";
    private final LocalServerSocket server;
    private volatile boolean running = true;
    private volatile boolean paused;
    private volatile AudioTrack track;
    private final Thread thread;

    AudioBridge(String socketName) throws IOException {
        server = new LocalServerSocket(socketName);
        thread = new Thread(this, "arcade-audio");
        thread.start();
    }

    void setPaused(boolean value) {
        paused = value;
        AudioTrack t = track;
        if (t != null) {
            if (value) t.pause();
            else t.play();
        }
    }

    void close() {
        running = false;
        try {
            server.close();
        } catch (IOException ignored) {
        }
        thread.interrupt();
    }

    @Override
    public void run() {
        while (running) {
            try (LocalSocket client = server.accept()) {
                client.setReceiveBufferSize(8192);
                play(client.getInputStream());
            } catch (IOException e) {
                if (running) Log.w(TAG, "audio connection ended: " + e.getMessage());
            }
        }
    }

    private void play(InputStream raw) throws IOException {
        DataInputStream in = new DataInputStream(raw);
        byte[] header = new byte[12];
        in.readFully(header);
        if (header[0] != 'A' || header[1] != 'F' || header[2] != 'A' || header[3] != '1') {
            throw new IOException("bad audio header");
        }
        int rate = le(header, 4);
        int channels = le(header, 8);
        int mask = channels == 1 ? AudioFormat.CHANNEL_OUT_MONO : AudioFormat.CHANNEL_OUT_STEREO;
        int min = AudioTrack.getMinBufferSize(rate, mask, AudioFormat.ENCODING_PCM_16BIT);
        AudioTrack t = new AudioTrack.Builder()
                .setAudioAttributes(new AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_GAME)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build())
                .setAudioFormat(new AudioFormat.Builder()
                        .setSampleRate(rate)
                        .setChannelMask(mask)
                        .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                        .build())
                .setBufferSizeInBytes(Math.max(min * 2, rate * channels * 2 / 10))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .build();
        track = t;
        if (!paused) t.play();
        Log.i(TAG, "playing " + rate + " Hz, " + channels + " ch");
        byte[] buffer = new byte[4096];
        int carry = 0;
        try {
            while (running) {
                int n = in.read(buffer, carry, buffer.length - carry);
                if (n < 0) break;
                int total = carry + n;
                int usable = total & ~3; // whole 16-bit stereo frames
                if (usable > 0) t.write(buffer, 0, usable);
                carry = total - usable;
                if (carry > 0) System.arraycopy(buffer, usable, buffer, 0, carry);
            }
        } finally {
            track = null;
            t.stop();
            t.release();
        }
    }

    private static int le(byte[] b, int o) {
        return (b[o] & 0xff) | (b[o + 1] & 0xff) << 8 | (b[o + 2] & 0xff) << 16 | (b[o + 3] & 0xff) << 24;
    }
}
