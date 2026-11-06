package com.bananapepper;

import android.content.ContentResolver;
import android.content.Context;
import android.database.Cursor;
import android.net.Uri;
import android.provider.DocumentsContract;

import java.io.File;
import java.io.FileInputStream;
import java.io.FileOutputStream;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.security.MessageDigest;
import java.util.ArrayList;
import java.util.List;

/**
 * Big Buck Hunter Pro: locating the user's files, importing them, and preparing
 * the hash-verified runtime copy (same transform as arcade-core::big_buck).
 */
final class BigBuck {
    static final String SOURCE_SHA256 = "c569677c4e4bcf3449939b6e9a44afd3346c153133391c24dc06c012a58c1244";
    private static final int CODE_OFFSET = 0x2d80;
    private static final int CODE_SIZE = 0x937c0;
    private static final int ENTRY = 0x0804ad80;
    private static final byte[] MASK = {(byte) 0x84, (byte) 0xeb, 0x67, (byte) 0xf1};
    private static final byte[] CRT_START = {0x31, (byte) 0xed, 0x5e, (byte) 0x89, (byte) 0xe1, (byte) 0x83,
            (byte) 0xe4, (byte) 0xf0, 0x50, 0x54, 0x52};

    interface Progress {
        void update(String message, int percent);
    }

    private BigBuck() {}

    static boolean looksLikeGame(File dir) {
        return dir != null && new File(dir, "game").isFile() && new File(dir, "bbh3data").isDirectory();
    }

    /** Imported copy (internal) or files placed in the app's external games folder. */
    static File findGame(Context context) {
        File internal = new File(Paths.games(context), "Big Buck Hunter Pro");
        if (looksLikeGame(internal)) return internal;
        File ext = Paths.externalGames(context);
        return ext == null ? null : search(ext, 3);
    }

    private static File search(File dir, int depth) {
        if (looksLikeGame(dir)) return dir;
        if (depth == 0) return null;
        File[] children = dir.listFiles();
        if (children == null) return null;
        for (File c : children) {
            if (c.isDirectory()) {
                File found = search(c, depth - 1);
                if (found != null) return found;
            }
        }
        return null;
    }

    static File preparedExecutable(Context context) {
        return new File(Paths.prepared(context), "big-buck-game");
    }

    /** Verifies the supplied executable and writes the runtime copy if needed. */
    static void prepare(Context context, File gameDir, Progress progress) throws IOException {
        File out = preparedExecutable(context);
        File stamp = new File(out.getPath() + ".source");
        File source = new File(gameDir, "game");
        String key = source.getAbsolutePath() + ":" + source.length() + ":" + source.lastModified();
        if (out.isFile() && stamp.isFile()
                && new String(RuntimeInstaller.readAll(new FileInputStream(stamp))).equals(key)) {
            return;
        }
        progress.update("Checking game files…", 0);
        byte[] bytes = RuntimeInstaller.readAll(new FileInputStream(source));
        String hash = sha256(bytes);
        if (!hash.equals(SOURCE_SHA256)) {
            throw new IOException("This Big Buck Hunter Pro build isn't supported (SHA-256 " + hash.substring(0, 16)
                    + "…). Only the tested 2008 Linux build can run.");
        }
        for (int i = 0; i < CODE_SIZE; i++) bytes[CODE_OFFSET + i] ^= MASK[i % 4];
        for (int i = 0; i < CRT_START.length; i++) {
            if (bytes[CODE_OFFSET + i] != CRT_START[i]) throw new IOException("Reconstructed entry validation failed");
        }
        bytes[24] = (byte) ENTRY;
        bytes[25] = (byte) (ENTRY >> 8);
        bytes[26] = (byte) (ENTRY >> 16);
        bytes[27] = (byte) (ENTRY >> 24);
        for (int i = 9; i < 16; i++) bytes[i] = 0;
        Paths.prepared(context).mkdirs();
        File tmp = new File(out.getPath() + ".preparing");
        try (OutputStream o = new FileOutputStream(tmp)) {
            o.write(bytes);
        }
        RuntimeInstaller.chmod(tmp, 0755);
        if (!tmp.renameTo(out)) throw new IOException("Cannot write prepared game");
        RuntimeInstaller.writeText(stamp, key);
    }

    /** Copies the supplied save/audit seed once; existing saves are kept. */
    static File saves(Context context, File gameDir) throws IOException {
        File dest = new File(Paths.saves(context), "bbh3user");
        seed(new File(gameDir, "bbh3user"), dest);
        return dest;
    }

    private static void seed(File from, File to) throws IOException {
        to.mkdirs();
        File[] children = from.listFiles();
        if (children == null) return;
        for (File c : children) {
            File t = new File(to, c.getName());
            if (RuntimeInstaller.isSymlink(c)) continue;
            if (c.isDirectory()) seed(c, t);
            else if (!t.exists()) RuntimeInstaller.copyFile(c, t);
        }
    }

    static String sha256(byte[] data) throws IOException {
        try {
            byte[] d = MessageDigest.getInstance("SHA-256").digest(data);
            StringBuilder sb = new StringBuilder();
            for (byte b : d) sb.append(String.format("%02x", b));
            return sb.toString();
        } catch (Exception e) {
            throw new IOException(e);
        }
    }

    // ---- Import through the system folder picker ------------------------------------

    private static final class Doc {
        final String id, name, mime;
        final long size;

        Doc(String id, String name, String mime, long size) {
            this.id = id;
            this.name = name;
            this.mime = mime;
            this.size = size;
        }

        boolean isDir() {
            return DocumentsContract.Document.MIME_TYPE_DIR.equals(mime);
        }
    }

    private static List<Doc> children(ContentResolver resolver, Uri tree, String parentId) {
        List<Doc> out = new ArrayList<>();
        Uri uri = DocumentsContract.buildChildDocumentsUriUsingTree(tree, parentId);
        String[] cols = {DocumentsContract.Document.COLUMN_DOCUMENT_ID, DocumentsContract.Document.COLUMN_DISPLAY_NAME,
                DocumentsContract.Document.COLUMN_MIME_TYPE, DocumentsContract.Document.COLUMN_SIZE};
        try (Cursor c = resolver.query(uri, cols, null, null, null)) {
            while (c != null && c.moveToNext()) {
                out.add(new Doc(c.getString(0), c.getString(1), c.getString(2), c.isNull(3) ? 0 : c.getLong(3)));
            }
        }
        return out;
    }

    private static boolean isGameDoc(ContentResolver r, Uri tree, String id) {
        boolean exe = false, data = false;
        for (Doc d : children(r, tree, id)) {
            if (d.name.equals("game") && !d.isDir()) exe = true;
            if (d.name.equals("bbh3data") && d.isDir()) data = true;
        }
        return exe && data;
    }

    private static String findGameDoc(ContentResolver r, Uri tree, String id, int depth) {
        if (isGameDoc(r, tree, id)) return id;
        if (depth == 0) return null;
        for (Doc d : children(r, tree, id)) {
            if (d.isDir()) {
                String found = findGameDoc(r, tree, d.id, depth - 1);
                if (found != null) return found;
            }
        }
        return null;
    }

    private static long totalSize(ContentResolver r, Uri tree, String id) {
        long sum = 0;
        for (Doc d : children(r, tree, id)) sum += d.isDir() ? totalSize(r, tree, d.id) : d.size;
        return sum;
    }

    /** Copies the chosen folder (or the Big Buck Hunter folder found inside it). */
    static void importTree(Context context, Uri tree, Progress progress) throws IOException {
        ContentResolver r = context.getContentResolver();
        String rootId = DocumentsContract.getTreeDocumentId(tree);
        progress.update("Looking for Big Buck Hunter Pro…", 0);
        String gameId = findGameDoc(r, tree, rootId, 3);
        if (gameId == null) {
            throw new IOException("No Big Buck Hunter Pro files found there. Choose the folder that contains "
                    + "\"game\" and \"bbh3data\" (for example \"Big Bug Hunter Pro\").");
        }
        long total = Math.max(1, totalSize(r, tree, gameId));
        File dest = new File(Paths.games(context), "Big Buck Hunter Pro");
        File staging = new File(Paths.games(context), "import.tmp");
        RuntimeInstaller.deleteRecursively(staging);
        staging.mkdirs();
        long[] done = {0};
        copyDoc(r, tree, gameId, staging, total, done, progress);
        RuntimeInstaller.deleteRecursively(dest);
        if (!staging.renameTo(dest)) throw new IOException("Cannot finish import");
        progress.update("Imported", 100);
    }

    private static void copyDoc(ContentResolver r, Uri tree, String id, File dir, long total, long[] done,
            Progress progress) throws IOException {
        byte[] buffer = new byte[1 << 17];
        for (Doc d : children(r, tree, id)) {
            if (d.name.contains("/") || d.name.equals("..") || d.name.equals(".")) continue;
            File target = new File(dir, d.name);
            if (d.isDir()) {
                target.mkdirs();
                copyDoc(r, tree, d.id, target, total, done, progress);
                continue;
            }
            Uri uri = DocumentsContract.buildDocumentUriUsingTree(tree, d.id);
            try (InputStream in = r.openInputStream(uri); OutputStream out = new FileOutputStream(target)) {
                if (in == null) throw new IOException("Cannot read " + d.name);
                int n;
                long lastTick = 0;
                while ((n = in.read(buffer)) > 0) {
                    out.write(buffer, 0, n);
                    done[0] += n;
                    if (done[0] - lastTick > (4 << 20)) {
                        lastTick = done[0];
                        progress.update("Copying game files… " + (done[0] >> 20) + " / " + (total >> 20) + " MB",
                                (int) (done[0] * 100 / total));
                    }
                }
            }
            progress.update("Copying game files… " + (done[0] >> 20) + " / " + (total >> 20) + " MB",
                    (int) (done[0] * 100 / total));
        }
    }
}
