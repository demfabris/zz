package dev.zz.android;

import android.text.TextUtils;
import android.view.View;
import android.view.inputmethod.BaseInputConnection;
import android.view.inputmethod.ExtractedText;
import android.view.inputmethod.ExtractedTextRequest;

final class ZzInputConnection extends BaseInputConnection {
    ZzInputConnection(View view) {
        super(view, false);
    }

    @Override
    public boolean commitText(CharSequence text, int cursor) {
        ZzNative.imeCommit(text.toString(), cursor);
        return true;
    }

    @Override
    public boolean setComposingText(CharSequence text, int cursor) {
        ZzNative.imeCompose(text.toString(), cursor);
        return true;
    }

    @Override
    public boolean setComposingRegion(int start, int end) {
        ZzNative.imeComposingRegion(start, end);
        return true;
    }

    @Override
    public boolean finishComposingText() {
        ZzNative.imeFinish();
        return true;
    }

    @Override
    public boolean deleteSurroundingText(int before, int after) {
        ZzNative.imeDelete(before, after);
        return true;
    }

    @Override
    public boolean deleteSurroundingTextInCodePoints(int before, int after) {
        ZzNative.imeDelete(before, after);
        return true;
    }

    @Override
    public CharSequence getTextBeforeCursor(int length, int flags) {
        int[] selection = ZzNative.imeSelection();
        if (selection == null) {
            return "";
        }
        String text = ZzNative.imeText(Math.max(0, selection[0] - length), selection[0]);
        return text == null ? "" : text;
    }

    @Override
    public CharSequence getTextAfterCursor(int length, int flags) {
        int[] selection = ZzNative.imeSelection();
        if (selection == null) {
            return "";
        }
        String text = ZzNative.imeText(selection[1], selection[1] + length);
        return text == null ? "" : text;
    }

    @Override
    public CharSequence getSelectedText(int flags) {
        int[] selection = ZzNative.imeSelection();
        if (selection == null || selection[0] == selection[1]) {
            return null;
        }
        return ZzNative.imeText(selection[0], selection[1]);
    }

    @Override
    public int getCursorCapsMode(int modes) {
        CharSequence before = getTextBeforeCursor(64, 0);
        return TextUtils.getCapsMode(before, before.length(), modes);
    }

    @Override
    public boolean setSelection(int start, int end) {
        ZzNative.imeSetSelection(start, end);
        return true;
    }

    @Override
    public boolean performEditorAction(int action) {
        ZzNative.imeAction(action);
        return true;
    }

    @Override
    public boolean performContextMenuAction(int id) {
        return ZzNative.imeMenuAction(id);
    }

    @Override
    public ExtractedText getExtractedText(ExtractedTextRequest request, int flags) {
        return null;
    }

    @Override
    public boolean beginBatchEdit() {
        return true;
    }

    @Override
    public boolean endBatchEdit() {
        return true;
    }
}
