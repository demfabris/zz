package dev.zz.android;

import android.content.Context;
import android.view.KeyEvent;
import android.view.MotionEvent;
import android.view.SurfaceHolder;
import android.view.SurfaceView;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputConnection;

final class ZzView extends SurfaceView implements SurfaceHolder.Callback2 {
    ZzView(Context context) {
        super(context);
        getHolder().addCallback(this);
        setFocusable(true);
        setFocusableInTouchMode(true);
        requestFocus();
    }

    @Override
    public void surfaceCreated(SurfaceHolder holder) {
        ZzNative.surfaceCreated(holder.getSurface());
    }

    @Override
    public void surfaceChanged(SurfaceHolder holder, int format, int width, int height) {
        ZzNative.surfaceChanged(holder.getSurface(), width, height);
    }

    @Override
    public void surfaceRedrawNeeded(SurfaceHolder holder) {
        ZzNative.surfaceRedrawNeeded();
    }

    @Override
    public void surfaceDestroyed(SurfaceHolder holder) {
        ZzNative.surfaceDestroyed();
    }

    @Override
    public boolean onTouchEvent(MotionEvent event) {
        int count = event.getPointerCount();
        int[] ids = new int[count];
        float[] xs = new float[count];
        float[] ys = new float[count];
        int[] tools = new int[count];
        for (int index = 0; index < count; index++) {
            ids[index] = event.getPointerId(index);
            xs[index] = event.getX(index);
            ys[index] = event.getY(index);
            tools[index] = event.getToolType(index);
        }
        ZzNative.touch(event.getActionMasked(), event.getActionIndex(), ids, xs, ys, tools,
                event.getButtonState(), event.getMetaState());
        return true;
    }

    @Override
    public boolean onGenericMotionEvent(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_HOVER_ENTER:
            case MotionEvent.ACTION_HOVER_MOVE:
            case MotionEvent.ACTION_HOVER_EXIT:
            case MotionEvent.ACTION_SCROLL:
                return ZzNative.pointer(event.getActionMasked(), event.getX(), event.getY(),
                        event.getAxisValue(MotionEvent.AXIS_HSCROLL), event.getAxisValue(MotionEvent.AXIS_VSCROLL),
                        event.getButtonState(), event.getMetaState());
            default:
                return super.onGenericMotionEvent(event);
        }
    }

    @Override
    public boolean dispatchKeyEvent(KeyEvent event) {
        int code = event.getKeyCode();
        switch (code) {
            case KeyEvent.KEYCODE_BACK:
            case KeyEvent.KEYCODE_HOME:
            case KeyEvent.KEYCODE_VOLUME_UP:
            case KeyEvent.KEYCODE_VOLUME_DOWN:
            case KeyEvent.KEYCODE_VOLUME_MUTE:
            case KeyEvent.KEYCODE_POWER:
            case KeyEvent.KEYCODE_APP_SWITCH:
                return super.dispatchKeyEvent(event);
            default:
                break;
        }
        if (event.getAction() == KeyEvent.ACTION_MULTIPLE && code == KeyEvent.KEYCODE_UNKNOWN) {
            String characters = event.getCharacters();
            if (characters != null) {
                ZzNative.imeCommit(characters, 1);
            }
            return true;
        }
        int meta = event.getMetaState();
        if (ZzNative.key(event.getAction(), code, event.getScanCode(), meta, event.getUnicodeChar(meta),
                event.getUnicodeChar(0), event.getRepeatCount())) {
            return true;
        }
        return super.dispatchKeyEvent(event);
    }

    @Override
    public boolean onCheckIsTextEditor() {
        return true;
    }

    @Override
    public InputConnection onCreateInputConnection(EditorInfo info) {
        int[] editor = ZzNative.editorInfo();
        info.inputType = editor[0];
        info.imeOptions = editor[1];
        info.initialSelStart = editor[2];
        info.initialSelEnd = editor[3];
        return new ZzInputConnection(this);
    }
}
