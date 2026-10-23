package dev.zz.android;

import android.view.Surface;

final class ZzNative {
    private ZzNative() {}

    static native void onCreate(
            ZzActivity activity, ZzView view, String filesDir, String cacheDir, String url, String[] environment);

    static native void onDestroy();

    static native void onResume();

    static native void onPause();

    static native void onConfiguration(
            float density,
            float fontScale,
            boolean night,
            int smallestWidthDp,
            boolean hardwareKeyboard,
            float refreshRate,
            boolean reduceMotion,
            boolean highContrast);

    static native void onUrl(String url);

    static native void onBack();

    static native void surfaceCreated(Surface surface);

    static native void surfaceChanged(Surface surface, int width, int height);

    static native void surfaceRedrawNeeded();

    static native void surfaceDestroyed();

    static native void insets(int top, int left, int bottom, int right, int ime, int imeRest, float cornerRadius);

    static native void touch(int action, int index, int[] ids, float[] xs, float[] ys, int[] tools, int buttons, int meta);

    static native boolean pointer(int action, float x, float y, float scrollX, float scrollY, int buttons, int meta);

    static native boolean key(int action, int keyCode, int scanCode, int meta, int character, int plainCharacter, int repeat);

    static native void focusChanged(boolean focused);

    static native int[] editorInfo();

    static native void imeCommit(String text, int cursor);

    static native void imeCompose(String text, int cursor);

    static native void imeComposingRegion(int start, int end);

    static native void imeFinish();

    static native void imeDelete(int before, int after);

    static native String imeText(int start, int end);

    static native int[] imeSelection();

    static native void imeSetSelection(int start, int end);

    static native void imeAction(int action);

    static native boolean imeMenuAction(int id);

    static native void keyRow(int index, boolean selected);
}
