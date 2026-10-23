package dev.zz.android;

import android.app.Activity;
import android.app.UiModeManager;
import android.content.ClipData;
import android.content.ClipDescription;
import android.content.ClipboardManager;
import android.content.Intent;
import android.content.res.ColorStateList;
import android.content.res.Configuration;
import android.graphics.Canvas;
import android.graphics.Color;
import android.graphics.ColorFilter;
import android.graphics.Insets;
import android.graphics.Paint;
import android.graphics.Path;
import android.graphics.PixelFormat;
import android.graphics.Rect;
import android.graphics.drawable.Drawable;
import android.graphics.drawable.GradientDrawable;
import android.graphics.drawable.RippleDrawable;
import android.net.Uri;
import android.os.Build;
import android.os.Bundle;
import android.provider.Settings;
import android.util.TypedValue;
import android.view.ActionMode;
import android.view.Display;
import android.view.Gravity;
import android.view.Menu;
import android.view.MenuItem;
import android.view.RoundedCorner;
import android.view.View;
import android.view.WindowInsets;
import android.view.WindowInsetsAnimation;
import android.view.WindowInsetsController;
import android.view.WindowManager;
import android.view.accessibility.AccessibilityEvent;
import android.view.accessibility.AccessibilityManager;
import android.view.inputmethod.InputMethodManager;
import android.widget.FrameLayout;
import android.widget.ImageView;
import android.widget.LinearLayout;
import android.widget.TextView;
import android.window.OnBackInvokedCallback;
import android.window.OnBackInvokedDispatcher;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.util.List;

public final class ZzActivity extends Activity {
    static {
        System.loadLibrary("zz_android");
    }

    private static final String[] KEY_ROW_LABELS = {
        "esc", "tab", "ctrl", "alt", "←", "↓", "↑", "→", null,
    };
    private static final String[] KEY_ROW_NAMES = {
        "Escape", "Tab", "Control", "Alt", "Left", "Down", "Up", "Right", "Hide keyboard",
    };
    private static final int KEY_ROW_HEIGHT = 48;

    ZzView view;
    FrameLayout root;
    private WindowInsets lastInsets;
    private int imeNow;
    private int imeRest;
    private int imeFull;
    private boolean imeAnimating;
    private LinearLayout keyRow;
    private View[] keyRowKeys;
    private boolean keyRowWanted;
    private boolean keyboardWanted;
    private ActionMode textMenu;
    private boolean backRegistered;
    private final OnBackInvokedCallback back = ZzNative::onBack;

    @Override
    protected void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().setDecorFitsSystemWindows(false);
        getWindow().setStatusBarColor(Color.TRANSPARENT);
        getWindow().setNavigationBarColor(Color.TRANSPARENT);
        getWindow().setNavigationBarContrastEnforced(false);
        getWindow().getAttributes().layoutInDisplayCutoutMode =
                WindowManager.LayoutParams.LAYOUT_IN_DISPLAY_CUTOUT_MODE_ALWAYS;
        root = new FrameLayout(this) {
            @Override
            public boolean requestSendAccessibilityEvent(View child, AccessibilityEvent event) {
                AccessibilityManager accessibility = getSystemService(AccessibilityManager.class);
                return accessibility != null && accessibility.isEnabled()
                        && super.requestSendAccessibilityEvent(child, event);
            }
        };
        view = new ZzView(this);
        root.addView(view, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));
        setContentView(root);
        root.getViewTreeObserver().addOnGlobalFocusChangeListener((was, now) -> resendInsets());
        root.setOnApplyWindowInsetsListener((target, insets) -> {
            lastInsets = insets;
            imeRest = insets.getInsets(WindowInsets.Type.ime()).bottom;
            if (!imeAnimating) {
                imeNow = imeRest;
            }
            sendInsets(insets);
            return insets;
        });
        root.setWindowInsetsAnimationCallback(
                new WindowInsetsAnimation.Callback(WindowInsetsAnimation.Callback.DISPATCH_MODE_STOP) {
                    @Override
                    public void onPrepare(WindowInsetsAnimation animation) {
                        if ((animation.getTypeMask() & WindowInsets.Type.ime()) != 0) {
                            imeAnimating = true;
                        }
                    }

                    @Override
                    public WindowInsets onProgress(WindowInsets insets, List<WindowInsetsAnimation> running) {
                        for (WindowInsetsAnimation animation : running) {
                            if ((animation.getTypeMask() & WindowInsets.Type.ime()) != 0) {
                                imeNow = insets.getInsets(WindowInsets.Type.ime()).bottom;
                                sendInsets(insets);
                                break;
                            }
                        }
                        return insets;
                    }

                    @Override
                    public void onEnd(WindowInsetsAnimation animation) {
                        if ((animation.getTypeMask() & WindowInsets.Type.ime()) != 0) {
                            imeAnimating = false;
                            imeNow = imeRest;
                            resendInsets();
                        }
                    }
                });
        sendConfiguration(getResources().getConfiguration());
        ZzNative.onCreate(this, view, getFilesDir().getAbsolutePath(), getCacheDir().getAbsolutePath(),
                urlOf(getIntent()), environmentOf(getIntent()));
    }

    @Override
    protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        String url = urlOf(intent);
        if (url != null) {
            ZzNative.onUrl(url);
        }
    }

    @Override
    protected void onResume() {
        super.onResume();
        sendConfiguration(getResources().getConfiguration());
        ZzNative.onResume();
    }

    @Override
    protected void onPause() {
        ZzNative.onPause();
        super.onPause();
    }

    @Override
    protected void onDestroy() {
        ZzNative.onDestroy();
        super.onDestroy();
    }

    @Override
    public void onConfigurationChanged(Configuration configuration) {
        super.onConfigurationChanged(configuration);
        sendConfiguration(configuration);
        if (keyRow != null) {
            boolean[] latched = {keyRowKeys[2].isActivated(), keyRowKeys[3].isActivated()};
            root.removeView(keyRow);
            keyRow = null;
            resendInsets();
            if (keyRow != null) {
                keyRowKeys[2].setActivated(latched[0]);
                keyRowKeys[3].setActivated(latched[1]);
            }
        }
    }

    @Override
    public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        ZzNative.focusChanged(focused);
    }

    private static String[] environmentOf(Intent intent) {
        Bundle extras = intent == null ? null : intent.getExtras();
        if (extras == null) {
            return new String[0];
        }
        java.util.ArrayList<String> pairs = new java.util.ArrayList<>();
        for (String key : extras.keySet()) {
            Object value = extras.get(key);
            if (value instanceof String && (key.startsWith("ZZ_") || key.startsWith("GPUI_"))) {
                pairs.add(key);
                pairs.add((String) value);
            }
        }
        return pairs.toArray(new String[0]);
    }

    private static String urlOf(Intent intent) {
        if (intent == null || !Intent.ACTION_VIEW.equals(intent.getAction()) || intent.getData() == null) {
            return null;
        }
        return intent.getData().toString();
    }

    private void sendInsets(WindowInsets insets) {
        Insets bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.displayCutout());
        float radius = 0;
        if (Build.VERSION.SDK_INT >= 31) {
            RoundedCorner corner = insets.getRoundedCorner(RoundedCorner.POSITION_BOTTOM_LEFT);
            if (corner != null) {
                radius = corner.getRadius();
            }
        }
        if (imeRest > 0) {
            imeFull = imeRest;
        }
        placeKeyRow(bars);
        int row = keyRowActive() ? dp(KEY_ROW_HEIGHT) : 0;
        int now = imeFull > 0 ? imeNow + row * Math.min(imeNow, imeFull) / imeFull : imeNow;
        int rest = imeRest > 0 ? imeRest + row : 0;
        ZzNative.insets(bars.top, bars.left, bars.bottom, bars.right, now, rest, radius);
    }

    private void resendInsets() {
        if (lastInsets != null) {
            sendInsets(lastInsets);
        }
    }

    private int dp(float value) {
        return Math.round(value * getResources().getDisplayMetrics().density);
    }

    private boolean keyRowActive() {
        return keyRowWanted && view.isFocused();
    }

    void setKeyRow(boolean wanted) {
        if (keyRowWanted != wanted) {
            keyRowWanted = wanted;
            resendInsets();
        }
    }

    void clearKeyRowLatch() {
        if (keyRowKeys != null) {
            for (View key : keyRowKeys) {
                key.setActivated(false);
            }
        }
    }

    private void placeKeyRow(Insets bars) {
        boolean shown = keyRowActive() && imeNow > 0;
        if (shown && keyRow == null) {
            keyRow = buildKeyRow();
            root.addView(keyRow, new FrameLayout.LayoutParams(
                    FrameLayout.LayoutParams.MATCH_PARENT, dp(KEY_ROW_HEIGHT), Gravity.BOTTOM));
        }
        if (keyRow == null) {
            return;
        }
        keyRow.setVisibility(shown ? View.VISIBLE : View.GONE);
        keyRow.setPadding(dp(8) + bars.left, 0, dp(8) + bars.right, 0);
        FrameLayout.LayoutParams params = (FrameLayout.LayoutParams) keyRow.getLayoutParams();
        if (params.bottomMargin != imeNow) {
            params.bottomMargin = imeNow;
            keyRow.setLayoutParams(params);
        }
    }

    private LinearLayout buildKeyRow() {
        boolean night = (getResources().getConfiguration().uiMode & Configuration.UI_MODE_NIGHT_MASK)
                == Configuration.UI_MODE_NIGHT_YES;
        int background;
        int fill;
        int text;
        int accent;
        int onAccent;
        if (Build.VERSION.SDK_INT >= 31) {
            background = getColor(night ? android.R.color.system_neutral1_900 : android.R.color.system_neutral1_100);
            fill = getColor(night ? android.R.color.system_neutral1_800 : android.R.color.system_neutral1_10);
            text = getColor(night ? android.R.color.system_neutral1_50 : android.R.color.system_neutral1_900);
            accent = getColor(night ? android.R.color.system_accent1_200 : android.R.color.system_accent1_600);
            onAccent = getColor(night ? android.R.color.system_accent1_800 : android.R.color.system_accent1_0);
        } else {
            background = night ? 0xff1f1f1f : 0xffeceff1;
            fill = night ? 0xff3c3c3c : 0xffffffff;
            text = night ? 0xffe8e8e8 : 0xff1f1f1f;
            accent = night ? 0xffa8c7fa : 0xff0b57d0;
            onAccent = night ? 0xff062e6f : 0xffffffff;
        }
        int[][] states = {{android.R.attr.state_activated}, {}};
        ColorStateList fills = new ColorStateList(states, new int[] {accent, fill});
        ColorStateList ink = new ColorStateList(states, new int[] {onAccent, text});
        ColorStateList ripple = ColorStateList.valueOf((text & 0x00ffffff) | 0x33000000);
        LinearLayout row = new LinearLayout(this);
        row.setOrientation(LinearLayout.HORIZONTAL);
        row.setGravity(Gravity.CENTER_VERTICAL);
        row.setBackgroundColor(background);
        row.setClickable(true);
        row.setFocusable(false);
        keyRowKeys = new View[KEY_ROW_NAMES.length];
        for (int index = 0; index < KEY_ROW_NAMES.length; index++) {
            View key;
            if (KEY_ROW_LABELS[index] != null) {
                TextView label = new TextView(this);
                label.setText(KEY_ROW_LABELS[index]);
                label.setTextColor(ink);
                label.setTextSize(TypedValue.COMPLEX_UNIT_SP, KEY_ROW_LABELS[index].length() == 1 ? 18 : 15);
                label.setGravity(Gravity.CENTER);
                key = label;
            } else {
                ImageView icon = new ImageView(this);
                icon.setImageDrawable(new Chevron(ink, dp(20), dp(2)));
                icon.setScaleType(ImageView.ScaleType.CENTER);
                key = icon;
                row.addView(new View(this), new LinearLayout.LayoutParams(0, 1, 1f));
            }
            GradientDrawable shape = new GradientDrawable();
            shape.setCornerRadius(dp(8));
            shape.setColor(fills);
            key.setBackground(new RippleDrawable(ripple, shape, null));
            key.setContentDescription(KEY_ROW_NAMES[index]);
            key.setMinimumWidth(dp(48));
            key.setPadding(dp(12), 0, dp(12), 0);
            final int which = index;
            final boolean latch = index == 2 || index == 3;
            key.setOnClickListener(target -> {
                if (latch) {
                    target.setActivated(!target.isActivated());
                }
                ZzNative.keyRow(which, target.isActivated());
            });
            key.setFocusable(false);
            LinearLayout.LayoutParams params =
                    new LinearLayout.LayoutParams(LinearLayout.LayoutParams.WRAP_CONTENT, dp(36));
            if (index < 7) {
                params.setMarginEnd(dp(8));
            }
            row.addView(key, params);
            keyRowKeys[index] = key;
        }
        return row;
    }

    private static final class Chevron extends Drawable {
        private final Paint paint = new Paint(Paint.ANTI_ALIAS_FLAG);
        private final Path path = new Path();
        private final ColorStateList ink;
        private final int size;

        Chevron(ColorStateList ink, int size, float stroke) {
            this.ink = ink;
            this.size = size;
            paint.setColor(ink.getDefaultColor());
            paint.setStyle(Paint.Style.STROKE);
            paint.setStrokeWidth(stroke);
            paint.setStrokeCap(Paint.Cap.ROUND);
            paint.setStrokeJoin(Paint.Join.ROUND);
        }

        @Override
        public int getIntrinsicWidth() {
            return size;
        }

        @Override
        public int getIntrinsicHeight() {
            return size;
        }

        @Override
        public boolean isStateful() {
            return true;
        }

        @Override
        protected boolean onStateChange(int[] state) {
            int color = ink.getColorForState(state, ink.getDefaultColor());
            if (color == paint.getColor()) {
                return false;
            }
            paint.setColor(color);
            return true;
        }

        @Override
        public void draw(Canvas canvas) {
            Rect bounds = getBounds();
            float x = bounds.exactCenterX();
            float y = bounds.exactCenterY();
            float width = bounds.width() * 0.3f;
            float height = bounds.height() * 0.15f;
            path.rewind();
            path.moveTo(x - width, y - height);
            path.lineTo(x, y + height);
            path.lineTo(x + width, y - height);
            canvas.drawPath(path, paint);
        }

        @Override
        public void setAlpha(int alpha) {
            paint.setAlpha(alpha);
        }

        @Override
        public void setColorFilter(ColorFilter filter) {
            paint.setColorFilter(filter);
        }

        @Override
        public int getOpacity() {
            return PixelFormat.TRANSLUCENT;
        }
    }

    private void sendConfiguration(Configuration configuration) {
        Display display = getDisplay();
        float refresh = 60;
        if (display != null) {
            Display.Mode current = display.getMode();
            refresh = current.getRefreshRate();
            for (Display.Mode mode : display.getSupportedModes()) {
                if (mode.getPhysicalWidth() == current.getPhysicalWidth()
                        && mode.getPhysicalHeight() == current.getPhysicalHeight()) {
                    refresh = Math.max(refresh, mode.getRefreshRate());
                }
            }
        }
        boolean reduceMotion = Settings.Global.getFloat(getContentResolver(),
                Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f;
        boolean highContrast = false;
        if (Build.VERSION.SDK_INT >= 34) {
            UiModeManager modes = getSystemService(UiModeManager.class);
            highContrast = modes != null && modes.getContrast() > 0.25f;
        }
        ZzNative.onConfiguration(
                getResources().getDisplayMetrics().density,
                configuration.fontScale,
                (configuration.uiMode & Configuration.UI_MODE_NIGHT_MASK) == Configuration.UI_MODE_NIGHT_YES,
                configuration.smallestScreenWidthDp,
                configuration.keyboard == Configuration.KEYBOARD_QWERTY
                        && configuration.hardKeyboardHidden == Configuration.HARDKEYBOARDHIDDEN_NO,
                refresh,
                reduceMotion,
                highContrast);
    }

    void setKeyboardVisible(boolean visible) {
        WindowInsetsController controller = getWindow().getInsetsController();
        InputMethodManager input = getSystemService(InputMethodManager.class);
        keyboardWanted = visible;
        if (visible) {
            view.requestFocus();
            controller.show(WindowInsets.Type.ime());
            if (input != null) {
                input.showSoftInput(view, 0);
            }
        } else {
            controller.hide(WindowInsets.Type.ime());
        }
    }

    void reclaimFocus() {
        view.requestFocus();
        setKeyboardVisible(keyboardWanted);
    }

    void restartInput() {
        InputMethodManager input = getSystemService(InputMethodManager.class);
        if (input != null) {
            input.restartInput(view);
        }
    }

    void updateSelection(int start, int end, int composeStart, int composeEnd) {
        InputMethodManager input = getSystemService(InputMethodManager.class);
        if (input != null) {
            input.updateSelection(view, start, end, composeStart, composeEnd);
        }
    }

    void setKeepScreenOn(boolean on) {
        if (on) {
            getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        } else {
            getWindow().clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        }
    }

    void setLightSystemBars(boolean light) {
        int mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS
                | WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS;
        getWindow().getInsetsController().setSystemBarsAppearance(light ? mask : 0, mask);
    }

    void setBackEnabled(boolean enabled) {
        if (Build.VERSION.SDK_INT < 33 || enabled == backRegistered) {
            return;
        }
        backRegistered = enabled;
        if (enabled) {
            getOnBackInvokedDispatcher().registerOnBackInvokedCallback(
                    OnBackInvokedDispatcher.PRIORITY_DEFAULT, back);
        } else {
            getOnBackInvokedDispatcher().unregisterOnBackInvokedCallback(back);
        }
    }

    @Override
    @SuppressWarnings("deprecation")
    public void onBackPressed() {
        if (backRegistered) {
            ZzNative.onBack();
        } else {
            super.onBackPressed();
        }
    }

    void openUrl(String url) {
        try {
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
        } catch (RuntimeException ignored) {
        }
    }

    void setClipboard(String text, byte[] image, String type) {
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        if (clipboard == null) {
            return;
        }
        ClipData clip = null;
        if (image != null && type != null) {
            try {
                File directory = new File(getCacheDir(), "clipboard");
                directory.mkdirs();
                File[] old = directory.listFiles();
                if (old != null) {
                    for (File file : old) {
                        file.delete();
                    }
                }
                String extension = type.equals("image/png") ? "png" : "jpg";
                File file = new File(directory, "image-" + System.currentTimeMillis() + "." + extension);
                try (FileOutputStream out = new FileOutputStream(file)) {
                    out.write(image);
                }
                Uri uri = ZzFileProvider.uriFor(this, file);
                clip = new ClipData("zz", new String[] {type}, new ClipData.Item(text, null, uri));
                if (text != null) {
                    clip.addItem(new ClipData.Item(text));
                }
            } catch (Exception ignored) {
                clip = null;
            }
        }
        if (clip == null) {
            clip = ClipData.newPlainText("zz", text == null ? "" : text);
        }
        clipboard.setPrimaryClip(clip);
    }

    String clipboardText() {
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        if (clipboard == null || !clipboard.hasPrimaryClip()) {
            return null;
        }
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null) {
            return null;
        }
        for (int index = 0; index < clip.getItemCount(); index++) {
            ClipData.Item item = clip.getItemAt(index);
            if (item.getText() != null) {
                return item.getText().toString();
            }
            if (item.getUri() == null && item.getIntent() == null) {
                CharSequence text = item.coerceToText(this);
                if (text != null) {
                    return text.toString();
                }
            }
        }
        return null;
    }

    Object[] clipboardImage() {
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        if (clipboard == null || !clipboard.hasPrimaryClip()) {
            return null;
        }
        ClipData clip = clipboard.getPrimaryClip();
        if (clip == null) {
            return null;
        }
        for (int index = 0; index < clip.getItemCount(); index++) {
            Uri uri = clip.getItemAt(index).getUri();
            if (uri == null) {
                continue;
            }
            String type = getContentResolver().getType(uri);
            if (type == null) {
                ClipDescription description = clip.getDescription();
                type = description.getMimeTypeCount() > 0 ? description.getMimeType(0) : null;
            }
            if (!"image/png".equals(type) && !"image/jpeg".equals(type)) {
                continue;
            }
            try (InputStream in = getContentResolver().openInputStream(uri)) {
                if (in == null) {
                    continue;
                }
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                byte[] buffer = new byte[65536];
                int read;
                while ((read = in.read(buffer)) > 0) {
                    out.write(buffer, 0, read);
                }
                return new Object[] {type, out.toByteArray()};
            } catch (Exception ignored) {
            }
        }
        return null;
    }

    boolean clipboardHasContent() {
        ClipboardManager clipboard = getSystemService(ClipboardManager.class);
        return clipboard != null && clipboard.hasPrimaryClip();
    }

    void showTextMenu(float x, float y) {
        if (textMenu != null) {
            textMenu.finish();
        }
        final Rect anchor = new Rect((int) x - 1, (int) y - 1, (int) x + 1, (int) y + 1);
        textMenu = view.startActionMode(new ActionMode.Callback2() {
            @Override
            public boolean onCreateActionMode(ActionMode mode, Menu menu) {
                menu.add(Menu.NONE, android.R.id.copy, 0, android.R.string.copy);
                if (clipboardHasContent()) {
                    menu.add(Menu.NONE, android.R.id.paste, 1, android.R.string.paste);
                }
                menu.add(Menu.NONE, android.R.id.selectAll, 2, android.R.string.selectAll);
                return true;
            }

            @Override
            public boolean onPrepareActionMode(ActionMode mode, Menu menu) {
                return false;
            }

            @Override
            public boolean onActionItemClicked(ActionMode mode, MenuItem item) {
                ZzNative.imeMenuAction(item.getItemId());
                mode.finish();
                return true;
            }

            @Override
            public void onDestroyActionMode(ActionMode mode) {
                textMenu = null;
            }

            @Override
            public void onGetContentRect(ActionMode mode, View target, Rect rect) {
                rect.set(anchor);
            }
        }, ActionMode.TYPE_FLOATING);
    }

    void hideTextMenu() {
        if (textMenu != null) {
            textMenu.finish();
        }
    }
}
