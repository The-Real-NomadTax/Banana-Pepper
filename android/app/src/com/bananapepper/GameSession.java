package com.bananapepper;

import android.content.Context;

import java.io.File;
import java.io.IOException;
import java.security.SecureRandom;
import java.util.ArrayList;
import java.util.List;
import java.util.Map;

/**
 * One running game: proot → Debian rootfs → Xvfb + afbridge + box64 game.
 * The whole tree is killed with the proot process (--kill-on-exit).
 */
final class GameSession {
    final File runDir;
    final File frameFile;
    final File logFile;
    final String inputSocket;
    final String audioSocket;
    private Process process;

    GameSession(Context context) {
        runDir = Paths.run(context);
        frameFile = new File(runDir, "frame.bin");
        logFile = new File(runDir, "session.log");
        String token = Long.toHexString(new SecureRandom().nextLong());
        inputSocket = "bananapepper." + token + ".input";
        audioSocket = "bananapepper." + token + ".audio";
    }

    void start(Context context, File gameDir, File prepared, File saves, boolean audio) throws IOException {
        runDir.mkdirs();
        frameFile.delete();
        File bin = Paths.bin(context);
        File tmp = Paths.tmp(context);
        tmp.mkdirs();
        File rootfs = Paths.rootfs(context);
        File opt = Paths.opt(context);
        List<String> cmd = new ArrayList<>();
        cmd.add(new File(bin, "proot").getPath());
        cmd.add("--kill-on-exit");
        cmd.add("--link2symlink");
        cmd.add("-0");
        cmd.add("-r");
        cmd.add(rootfs.getPath());
        bind(cmd, "/dev", "/dev");
        bind(cmd, "/proc", "/proc");
        bind(cmd, "/sys", "/sys");
        bind(cmd, tmp.getPath(), "/tmp");
        bind(cmd, tmp.getPath(), "/dev/shm");
        bind(cmd, gameDir.getPath(), "/game");
        bind(cmd, prepared.getPath(), "/game/game");
        bind(cmd, new File(gameDir, "bbh3data").getPath(), "/bbh3data");
        bind(cmd, saves.getPath(), "/bbh3user");
        bind(cmd, runDir.getPath(), "/run/af");
        bind(cmd, new File(opt, "libarcade_linux_shim.so").getPath(), "/opt/arcade/libarcade_linux_shim.so");
        bind(cmd, new File(opt, "run-big-buck.sh").getPath(), "/opt/arcade/run-big-buck.sh");
        cmd.add("-w");
        cmd.add("/game");
        cmd.add("/usr/bin/env");
        cmd.add("-i");
        cmd.add("PATH=/usr/local/bin:/usr/bin:/bin");
        cmd.add("LANG=C.UTF-8");
        cmd.add("AF_INPUT_SOCKET=" + inputSocket);
        cmd.add("AF_APP_PID=" + android.os.Process.myPid());
        if (audio) cmd.add("AF_AUDIO_SOCKET=" + audioSocket);
        cmd.add("/bin/bash");
        cmd.add("/opt/arcade/run-big-buck.sh");

        ProcessBuilder pb = new ProcessBuilder(cmd);
        Map<String, String> env = pb.environment();
        env.put("LD_LIBRARY_PATH", bin.getPath());
        env.put("PROOT_LOADER", new File(bin, "loader").getPath());
        env.put("PROOT_TMP_DIR", tmp.getPath());
        env.remove("LD_PRELOAD");
        pb.redirectErrorStream(true);
        pb.redirectOutput(logFile);
        process = pb.start();
    }

    private static void bind(List<String> cmd, String host, String guest) {
        cmd.add("-b");
        cmd.add(host + ":" + guest);
    }

    boolean isRunning() {
        return process != null && process.isAlive();
    }

    int exitCode() {
        return process == null || process.isAlive() ? -1 : process.exitValue();
    }

    void stop() {
        if (process != null) {
            process.destroy();
            try {
                if (!process.waitFor(2, java.util.concurrent.TimeUnit.SECONDS)) process.destroyForcibly();
            } catch (InterruptedException e) {
                process.destroyForcibly();
            }
        }
    }
}
