package dev.zz.android;

import android.content.ContentProvider;
import android.content.ContentValues;
import android.content.Context;
import android.database.Cursor;
import android.database.MatrixCursor;
import android.net.Uri;
import android.os.ParcelFileDescriptor;
import android.provider.OpenableColumns;
import java.io.File;
import java.io.FileNotFoundException;

public final class ZzFileProvider extends ContentProvider {
    static Uri uriFor(Context context, File file) {
        return new Uri.Builder()
                .scheme("content")
                .authority(context.getPackageName() + ".files")
                .appendPath(file.getParentFile().getName())
                .appendPath(file.getName())
                .build();
    }

    private File fileFor(Uri uri) throws FileNotFoundException {
        if (uri.getPathSegments().size() != 2 || !"clipboard".equals(uri.getPathSegments().get(0))) {
            throw new FileNotFoundException(uri.toString());
        }
        File file = new File(new File(getContext().getCacheDir(), "clipboard"), uri.getLastPathSegment());
        if (!file.isFile() || file.getName().contains("/")) {
            throw new FileNotFoundException(uri.toString());
        }
        return file;
    }

    @Override
    public boolean onCreate() {
        return true;
    }

    @Override
    public String getType(Uri uri) {
        String name = uri.getLastPathSegment();
        if (name == null) {
            return null;
        }
        if (name.endsWith(".png")) {
            return "image/png";
        }
        if (name.endsWith(".jpg")) {
            return "image/jpeg";
        }
        return "application/octet-stream";
    }

    @Override
    public ParcelFileDescriptor openFile(Uri uri, String mode) throws FileNotFoundException {
        return ParcelFileDescriptor.open(fileFor(uri), ParcelFileDescriptor.MODE_READ_ONLY);
    }

    @Override
    public Cursor query(Uri uri, String[] projection, String selection, String[] arguments, String order) {
        try {
            File file = fileFor(uri);
            MatrixCursor cursor = new MatrixCursor(new String[] {OpenableColumns.DISPLAY_NAME, OpenableColumns.SIZE});
            cursor.addRow(new Object[] {file.getName(), file.length()});
            return cursor;
        } catch (FileNotFoundException missing) {
            return null;
        }
    }

    @Override
    public Uri insert(Uri uri, ContentValues values) {
        return null;
    }

    @Override
    public int delete(Uri uri, String selection, String[] arguments) {
        return 0;
    }

    @Override
    public int update(Uri uri, ContentValues values, String selection, String[] arguments) {
        return 0;
    }
}
