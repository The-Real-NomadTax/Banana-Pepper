package com.arcadefoundry;

import android.net.LocalSocket;
import android.net.LocalSocketAddress;

import java.io.IOException;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.concurrent.LinkedBlockingQueue;
import java.util.concurrent.TimeUnit;

/** Sends pointer/key events to afbridge, which injects them into the game's X display. */
final class InputBridge implements Runnable {
    private final String socketName;
    private final LinkedBlockingQueue<String> queue = new LinkedBlockingQueue<>();
    private volatile boolean running = true;
    private final Thread thread;

    InputBridge(String socketName) {
        this.socketName = socketName;
        thread = new Thread(this, "arcade-input");
        thread.start();
    }

    void move(int x, int y) {
        queue.offer("m " + x + " " + y + "\n");
    }

    void button(int button, boolean down) {
        queue.offer("b " + button + " " + (down ? 1 : 0) + "\n");
    }

    /** X keysym name, e.g. "t", "1", "5", "Escape". */
    void key(String keysym, boolean down) {
        queue.offer("k " + keysym + " " + (down ? 1 : 0) + "\n");
    }

    void close() {
        running = false;
        thread.interrupt();
    }

    @Override
    public void run() {
        while (running) {
            try (LocalSocket socket = new LocalSocket()) {
                socket.connect(new LocalSocketAddress(socketName, LocalSocketAddress.Namespace.ABSTRACT));
                OutputStream out = socket.getOutputStream();
                while (running) {
                    String line = queue.poll(500, TimeUnit.MILLISECONDS);
                    if (line == null) continue;
                    StringBuilder batch = new StringBuilder(line);
                    String more;
                    while ((more = queue.poll()) != null) batch.append(more);
                    out.write(batch.toString().getBytes(StandardCharsets.US_ASCII));
                    out.flush();
                }
            } catch (IOException e) {
                // Bridge not up yet or game restarted; retry shortly.
                try {
                    Thread.sleep(250);
                } catch (InterruptedException ie) {
                    return;
                }
            } catch (InterruptedException e) {
                return;
            }
        }
    }
}
