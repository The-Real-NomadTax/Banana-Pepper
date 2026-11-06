package com.arcadefoundry;

import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.Paint;
import android.graphics.Rect;
import android.graphics.RectF;
import android.view.SurfaceHolder;
import android.view.SurfaceView;

import java.io.File;
import java.io.RandomAccessFile;
import java.nio.ByteOrder;
import java.nio.MappedByteBuffer;
import java.nio.channels.FileChannel;

/**
 * Draws frames that afbridge copies out of the game's X display into a shared
 * memory-mapped file (32-byte header + RGBA pixels), scaled to fit (4:3).
 */
final class GameView extends SurfaceView implements SurfaceHolder.Callback, Runnable {
    interface Listener {
        void onFirstFrame();
    }

    private static final int MAGIC = 0x42464641;
    private final File frameFile;
    private final Listener listener;
    private final Paint paint = new Paint(Paint.FILTER_BITMAP_FLAG);
    private volatile boolean running;
    private Thread thread;
    private volatile RectF gameRect = new RectF();
    private volatile int frameWidth = 640, frameHeight = 480;
    private boolean announced;
    /** Guards the surface between the render thread and surface callbacks. */
    private final Object surfaceLock = new Object();
    private boolean surfaceReady;

    GameView(Context context, File frameFile, Listener listener) {
        super(context);
        this.frameFile = frameFile;
        this.listener = listener;
        getHolder().addCallback(this);
    }

    /** Where the game picture is on screen, in view pixels. */
    RectF gameRect() {
        return gameRect;
    }

    int frameWidth() {
        return frameWidth;
    }

    int frameHeight() {
        return frameHeight;
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        synchronized (surfaceLock) {
            surfaceReady = true;
        }
        running = true;
        thread = new Thread(this, "arcade-render");
        thread.start();
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        layoutRect(width, height);
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        // No canvas may be locked once this returns: take the lock, then stop.
        synchronized (surfaceLock) {
            surfaceReady = false;
        }
        running = false;
        if (thread != null) {
            thread.interrupt();
            try {
                thread.join();
            } catch (InterruptedException ignored) {
            }
            thread = null;
        }
    }

    /** Draws one frame (or black when bitmap is null) if the surface is still alive. */
    private void present(Bitmap bitmap) {
        synchronized (surfaceLock) {
            SurfaceHolder holder = getHolder();
            if (!surfaceReady || !holder.getSurface().isValid()) return;
            Canvas canvas;
            try {
                canvas = holder.lockHardwareCanvas();
            } catch (Exception e) {
                return;
            }
            if (canvas == null) return;
            try {
                canvas.drawColor(Color.BLACK);
                if (bitmap != null) canvas.drawBitmap(bitmap, null, gameRect, paint);
            } finally {
                holder.unlockCanvasAndPost(canvas);
            }
        }
    }

    private void layoutRect(int width, int height) {
        float scale = Math.min(width / (float) frameWidth, height / (float) frameHeight);
        float w = frameWidth * scale, h = frameHeight * scale;
        float left = (width - w) / 2f, top = (height - h) / 2f;
        gameRect = new RectF(left, top, left + w, top + h);
    }

    @Override
    public void run() {
        MappedByteBuffer header = null, pixels = null;
        Bitmap bitmap = null;
        int lastSeq = -1;
        long nextOpenAttempt = 0;
        while (running) {
            try {
                if (pixels == null) {
                    long now = System.currentTimeMillis();
                    if (now < nextOpenAttempt) {
                        Thread.sleep(50);
                        continue;
                    }
                    nextOpenAttempt = now + 200;
                    if (!frameFile.isFile() || frameFile.length() < 32) {
                        drawBlank();
                        continue;
                    }
                    try (RandomAccessFile raf = new RandomAccessFile(frameFile, "r")) {
                        FileChannel ch = raf.getChannel();
                        header = ch.map(FileChannel.MapMode.READ_ONLY, 0, 32);
                        header.order(ByteOrder.LITTLE_ENDIAN);
                        if (header.getInt(0) != MAGIC) {
                            header = null;
                            continue;
                        }
                        frameWidth = header.getInt(4);
                        frameHeight = header.getInt(8);
                        long size = (long) frameWidth * frameHeight * 4;
                        if (raf.length() < 32 + size) {
                            header = null;
                            continue;
                        }
                        pixels = ch.map(FileChannel.MapMode.READ_ONLY, 32, size);
                    }
                    bitmap = Bitmap.createBitmap(frameWidth, frameHeight, Bitmap.Config.ARGB_8888);
                    layoutRect(getWidth(), getHeight());
                }
                int seq = header.getInt(12);
                if (seq == lastSeq) {
                    Thread.sleep(4);
                    continue;
                }
                lastSeq = seq;
                pixels.rewind();
                bitmap.copyPixelsFromBuffer(pixels);
                if (!announced && hasContent(bitmap)) {
                    announced = true;
                    listener.onFirstFrame();
                }
                present(bitmap);
            } catch (InterruptedException e) {
                return;
            } catch (Exception e) {
                pixels = null;
                header = null;
                try {
                    Thread.sleep(200);
                } catch (InterruptedException ie) {
                    return;
                }
            }
        }
    }

    private static boolean hasContent(Bitmap b) {
        int w = b.getWidth(), h = b.getHeight();
        for (int y = h / 8; y < h; y += h / 8) {
            for (int x = w / 16; x < w; x += w / 16) {
                if ((b.getPixel(x, y) & 0xffffff) != 0) return true;
            }
        }
        return false;
    }

    private void drawBlank() throws InterruptedException {
        present(null);
        Thread.sleep(100);
    }

    @SuppressWarnings("unused")
    private static Rect toRect(RectF r) {
        return new Rect(Math.round(r.left), Math.round(r.top), Math.round(r.right), Math.round(r.bottom));
    }
}
