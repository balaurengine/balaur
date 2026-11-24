package org.balaur.platform;

import android.content.ContentProvider;
import android.content.ContentResolver;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import android.webkit.MimeTypeMap;

import java.io.File;
import java.io.FileNotFoundException;
import java.io.IOException;
import java.util.Locale;

/**
 * The game's own files, read-only, as the content URIs a share hands on: what
 * AndroidX's FileProvider does, with no AndroidX. Only a file under the app's
 * data directory or its external files directory is served; the export names
 * the authority after the application id.
 */
public final class Files extends ContentProvider {
    private static final String AUTHORITY = ".balaur.files";
    private static final String READ = "r";
    private static final String ANY = "application/octet-stream";
    private static final String[] COLUMNS = {OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE};

    /** The content URI {@code file} is shared under. */
    static Uri uri(Context context, File file) {
        return new Uri.Builder()
                .scheme(ContentResolver.SCHEME_CONTENT)
                .authority(context.getPackageName() + AUTHORITY)
                .path(file.getAbsolutePath())
                .build();
    }

    /** A file's MIME type by its extension, or a byte stream's. */
    static String type(String path) {
        int dot = path.lastIndexOf('.');
        String extension = dot < 0 ? "" : path.substring(dot + 1).toLowerCase(Locale.ROOT);
        String type = MimeTypeMap.getSingleton().getMimeTypeFromExtension(extension);
        return type == null ? ANY : type;
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        if (!READ.equals(mode)) {
            throw new FileNotFoundException("shared files are read-only: " + uri);
        }
        return ParcelFileDescriptor.open(file(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection, String[] args, String order) {
        File file;
        try {
            file = file(uri);
        } catch (FileNotFoundException e) {
            return null;
        }
        String[] columns = projection == null ? COLUMNS : projection;
        Object[] row = new Object[columns.length];
        for (int i = 0; i < columns.length; i++) {
            if (OpenableColumns.DISPLAY_NAME.equals(columns[i])) {
                row[i] = file.getName();
            } else if (OpenableColumns.SIZE.equals(columns[i])) {
                row[i] = file.length();
            }
        }
        MatrixCursor cursor = new MatrixCursor(columns, 1);
        cursor.addRow(row);
        return cursor;
    }

    @Override
    public String getType(Uri uri) {
        String path = uri.getPath();
        return path == null ? ANY : type(path);
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        throw new UnsupportedOperationException("shared files are read-only");
    }

    @Override
    public int delete(Uri uri, String selection, String[] args) {
        throw new UnsupportedOperationException("shared files are read-only");
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] args) {
        throw new UnsupportedOperationException("shared files are read-only");
    }

    /** The file a URI names, when it is the game's own. */
    private File file(Uri uri) throws FileNotFoundException {
        Context context = getContext();
        String path = uri.getPath();
        if (context == null || path == null) {
            throw new FileNotFoundException(String.valueOf(uri));
        }
        try {
            File file = new File(path).getCanonicalFile();
            File[] roots = {new File(context.getApplicationInfo().dataDir), context.getExternalFilesDir(null)};
            for (File root : roots) {
                if (root != null && file.getPath().startsWith(root.getCanonicalPath() + File.separator)
                        && file.isFile()) {
                    return file;
                }
            }
        } catch (IOException e) {
            // An unreadable path is one this does not serve.
        }
        throw new FileNotFoundException(String.valueOf(uri));
    }
}
