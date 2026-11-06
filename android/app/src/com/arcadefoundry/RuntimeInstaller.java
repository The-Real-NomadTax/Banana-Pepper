package com.arcadefoundry;

import android.content.Context;
import android.content.res.AssetFileDescriptor;
import android.system.Os;

import java.io.BufferedInputStream;
import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.FilterInputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.nio.charset.StandardCharsets;
import java.util.zip.GZIPInputStream;

/**
 * Installs the bundled Linux runtime (proot + Debian arm64 rootfs with box64)
 * from APK assets into the app's private files directory.
 */
final class RuntimeInstaller {
    interface Progress {
        void update(String message, int percent);
    }

    private static final String VERSION_FILE = ".arcade-runtime-version";

    private RuntimeInstaller() {}

    static String bundledVersion(Context context) throws IOException {
        try (InputStream in = context.getAssets().open("runtime/version.txt")) {
            return new String(readAll(in), StandardCharsets.UTF_8).trim();
        }
    }

    static boolean isInstalled(Context context) {
        try {
            File marker = new File(Paths.rootfs(context), VERSION_FILE);
            if (!marker.isFile()) return false;
            String installed = new String(readAll(new FileInputStream(marker)), StandardCharsets.UTF_8).trim();
            return installed.equals(bundledVersion(context)) && toolsCurrent(context);
        } catch (IOException e) {
            return false;
        }
    }

    private static boolean toolsCurrent(Context context) throws IOException {
        File marker = new File(Paths.bin(context), VERSION_FILE);
        if (!marker.isFile()) return false;
        String v = new String(readAll(new FileInputStream(marker)), StandardCharsets.UTF_8).trim();
        return v.equals(bundledVersion(context) + "/" + BuildInfo.VERSION_CODE);
    }

    /** Small tools and scripts; refreshed whenever the app version changes. */
    static void installTools(Context context) throws IOException {
        File bin = Paths.bin(context);
        bin.mkdirs();
        for (String name : new String[] {"proot", "loader", "libtalloc.so.2", "libandroid-shmem.so"}) {
            copyAsset(context, "runtime/" + name, new File(bin, name), 0755);
        }
        File opt = Paths.opt(context);
        opt.mkdirs();
        copyAsset(context, "runtime/libarcade_linux_shim.so", new File(opt, "libarcade_linux_shim.so"), 0755);
        copyAsset(context, "runtime/run-big-buck.sh", new File(opt, "run-big-buck.sh"), 0755);
        Paths.tmp(context).mkdirs();
        writeText(new File(bin, VERSION_FILE), bundledVersion(context) + "/" + BuildInfo.VERSION_CODE);
    }

    static void install(Context context, Progress progress) throws IOException {
        String version = bundledVersion(context);
        File rootfs = Paths.rootfs(context);
        File marker = new File(rootfs, VERSION_FILE);
        boolean rootfsCurrent = false;
        if (marker.isFile()) {
            String installed = new String(readAll(new FileInputStream(marker)), StandardCharsets.UTF_8).trim();
            rootfsCurrent = installed.equals(version);
        }
        if (!rootfsCurrent) {
            progress.update("Removing old runtime…", 0);
            deleteRecursively(rootfs);
            File staging = new File(rootfs.getParentFile(), "rootfs.installing");
            deleteRecursively(staging);
            staging.mkdirs();
            long total;
            try (AssetFileDescriptor fd = context.getAssets().openFd("runtime/rootfs.tar.gz")) {
                total = fd.getLength();
            }
            CountingStream counted = new CountingStream(context.getAssets().open("runtime/rootfs.tar.gz"));
            try (InputStream tar = new GZIPInputStream(new BufferedInputStream(counted, 1 << 16), 1 << 16)) {
                extractTar(tar, staging, () -> {
                    int pct = total > 0 ? (int) (counted.count * 100 / total) : 0;
                    progress.update("Installing game runtime…", Math.min(99, pct));
                });
            }
            writeText(new File(staging, VERSION_FILE), version);
            if (!staging.renameTo(rootfs)) throw new IOException("Cannot activate runtime");
        }
        progress.update("Finishing…", 99);
        installTools(context);
        progress.update("Runtime ready", 100);
    }

    // ---- tar ---------------------------------------------------------------------

    private static void extractTar(InputStream in, File root, Runnable tick) throws IOException {
        byte[] header = new byte[512];
        byte[] buffer = new byte[1 << 16];
        String longName = null, longLink = null;
        int entries = 0;
        String canonicalRoot = root.getCanonicalPath() + File.separator;
        while (true) {
            if (!readFully(in, header)) break;
            if (isZero(header)) break;
            String name = longName != null ? longName : str(header, 0, 100);
            String prefix = str(header, 345, 155);
            if (longName == null && !prefix.isEmpty()) name = prefix + "/" + name;
            String link = longLink != null ? longLink : str(header, 157, 100);
            longName = null;
            longLink = null;
            char type = (char) header[156];
            long size = octal(header, 124, 12);
            int mode = (int) octal(header, 100, 8) & 0777;

            if (type == 'L' || type == 'K') {
                byte[] data = new byte[(int) size];
                readFully(in, data);
                skip(in, pad(size));
                String value = cstr(data);
                if (type == 'L') longName = value; else longLink = value;
                continue;
            }
            if (type == 'x' || type == 'g') {
                byte[] data = new byte[(int) size];
                readFully(in, data);
                skip(in, pad(size));
                if (type == 'x') {
                    String pathValue = paxValue(data, "path");
                    String linkValue = paxValue(data, "linkpath");
                    if (pathValue != null) longName = pathValue;
                    if (linkValue != null) longLink = linkValue;
                }
                continue;
            }
            while (name.startsWith("./")) name = name.substring(2);
            if (name.isEmpty() || name.equals(".")) {
                skip(in, size + pad(size));
                continue;
            }
            File target = new File(root, name);
            String canonicalParent = target.getParentFile().getCanonicalPath() + File.separator;
            if (!canonicalParent.startsWith(canonicalRoot) && !canonicalParent.equals(canonicalRoot)) {
                throw new IOException("Unsafe path in runtime archive: " + name);
            }
            target.getParentFile().mkdirs();
            switch (type) {
                case '5':
                    target.mkdirs();
                    chmod(target, mode | 0700);
                    break;
                case '2':
                    target.delete();
                    try {
                        Os.symlink(link, target.getPath());
                    } catch (Exception e) {
                        throw new IOException("symlink " + name + ": " + e.getMessage());
                    }
                    break;
                case '1': {
                    String linked = link;
                    while (linked.startsWith("./")) linked = linked.substring(2);
                    File source = new File(root, linked);
                    copyFile(source, target);
                    chmod(target, mode | 0600);
                    break;
                }
                case '0':
                case '\0':
                case '7': {
                    try (OutputStream out = new FileOutputStream(target)) {
                        long left = size;
                        while (left > 0) {
                            int n = in.read(buffer, 0, (int) Math.min(buffer.length, left));
                            if (n < 0) throw new IOException("Truncated runtime archive");
                            out.write(buffer, 0, n);
                            left -= n;
                        }
                    }
                    skip(in, pad(size));
                    chmod(target, mode | 0600);
                    break;
                }
                default:
                    // Device nodes, FIFOs: not needed inside the app sandbox.
                    skip(in, size + pad(size));
            }
            if ((++entries & 255) == 0) tick.run();
        }
    }

    private static String paxValue(byte[] data, String key) {
        String text = new String(data, StandardCharsets.UTF_8);
        int i = 0;
        while (i < text.length()) {
            int space = text.indexOf(' ', i);
            if (space < 0) break;
            int len;
            try {
                len = Integer.parseInt(text.substring(i, space));
            } catch (NumberFormatException e) {
                break;
            }
            String record = text.substring(space + 1, Math.min(text.length(), i + len - 1));
            int eq = record.indexOf('=');
            if (eq > 0 && record.substring(0, eq).equals(key)) return record.substring(eq + 1);
            i += len;
        }
        return null;
    }

    private static long pad(long size) {
        return (512 - (size % 512)) % 512;
    }

    private static boolean isZero(byte[] b) {
        for (byte x : b) if (x != 0) return false;
        return true;
    }

    private static String str(byte[] b, int off, int len) {
        int end = off;
        while (end < off + len && b[end] != 0) end++;
        return new String(b, off, end - off, StandardCharsets.UTF_8);
    }

    private static String cstr(byte[] b) {
        int end = 0;
        while (end < b.length && b[end] != 0) end++;
        return new String(b, 0, end, StandardCharsets.UTF_8);
    }

    private static long octal(byte[] b, int off, int len) {
        if ((b[off] & 0x80) != 0) { // base-256 (large files)
            long v = 0;
            for (int i = off + 1; i < off + len; i++) v = (v << 8) | (b[i] & 0xff);
            return v;
        }
        long v = 0;
        for (int i = off; i < off + len; i++) {
            byte c = b[i];
            if (c == 0 || c == ' ') {
                if (v != 0) break;
                continue;
            }
            v = v * 8 + (c - '0');
        }
        return v;
    }

    private static boolean readFully(InputStream in, byte[] b) throws IOException {
        int off = 0;
        while (off < b.length) {
            int n = in.read(b, off, b.length - off);
            if (n < 0) {
                if (off == 0) return false;
                throw new IOException("Truncated runtime archive");
            }
            off += n;
        }
        return true;
    }

    private static void skip(InputStream in, long n) throws IOException {
        byte[] scratch = new byte[8192];
        while (n > 0) {
            int r = in.read(scratch, 0, (int) Math.min(scratch.length, n));
            if (r < 0) throw new IOException("Truncated runtime archive");
            n -= r;
        }
    }

    // ---- helpers -------------------------------------------------------------------

    static void copyAsset(Context context, String asset, File dest, int mode) throws IOException {
        File tmp = new File(dest.getPath() + ".tmp");
        try (InputStream in = context.getAssets().open(asset); OutputStream out = new FileOutputStream(tmp)) {
            byte[] b = new byte[1 << 16];
            int n;
            while ((n = in.read(b)) > 0) out.write(b, 0, n);
        }
        chmod(tmp, mode);
        if (!tmp.renameTo(dest)) throw new IOException("Cannot install " + dest);
    }

    static void copyFile(File from, File to) throws IOException {
        try (InputStream in = new FileInputStream(from); OutputStream out = new FileOutputStream(to)) {
            byte[] b = new byte[1 << 16];
            int n;
            while ((n = in.read(b)) > 0) out.write(b, 0, n);
        }
    }

    static void chmod(File f, int mode) {
        try {
            Os.chmod(f.getPath(), mode);
        } catch (Exception ignored) {
        }
    }

    static void writeText(File f, String text) throws IOException {
        try (OutputStream out = new FileOutputStream(f)) {
            out.write(text.getBytes(StandardCharsets.UTF_8));
        }
    }

    static byte[] readAll(InputStream in) throws IOException {
        try (InputStream s = in) {
            java.io.ByteArrayOutputStream out = new java.io.ByteArrayOutputStream();
            byte[] b = new byte[8192];
            int n;
            while ((n = s.read(b)) > 0) out.write(b, 0, n);
            return out.toByteArray();
        }
    }

    static void deleteRecursively(File f) {
        if (!f.exists() && !isSymlink(f)) return;
        if (f.isDirectory() && !isSymlink(f)) {
            File[] children = f.listFiles();
            if (children != null) for (File c : children) deleteRecursively(c);
        }
        f.delete();
    }

    static boolean isSymlink(File f) {
        try {
            return android.system.OsConstants.S_ISLNK(Os.lstat(f.getPath()).st_mode);
        } catch (Exception e) {
            return false;
        }
    }

    private static final class CountingStream extends FilterInputStream {
        volatile long count;

        CountingStream(InputStream in) {
            super(in);
        }

        @Override
        public int read() throws IOException {
            int b = super.read();
            if (b >= 0) count++;
            return b;
        }

        @Override
        public int read(byte[] b, int off, int len) throws IOException {
            int n = super.read(b, off, len);
            if (n > 0) count += n;
            return n;
        }
    }
}
