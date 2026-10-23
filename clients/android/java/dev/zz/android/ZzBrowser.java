package dev.zz.android;

import android.annotation.SuppressLint;
import android.content.Context;
import android.graphics.Bitmap;
import android.graphics.Canvas;
import android.graphics.Path;
import android.graphics.Rect;
import android.graphics.RectF;
import android.os.Handler;
import android.os.Looper;
import android.os.Message;
import android.view.Gravity;
import android.view.MotionEvent;
import android.view.PixelCopy;
import android.view.View;
import android.webkit.JavascriptInterface;
import android.webkit.RenderProcessGoneDetail;
import android.webkit.WebChromeClient;
import android.webkit.WebResourceError;
import android.webkit.WebResourceRequest;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.FrameLayout;
import android.widget.ProgressBar;
import androidx.webkit.ProfileStore;
import androidx.webkit.ProxyConfig;
import androidx.webkit.ProxyController;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;
import java.io.ByteArrayOutputStream;
import java.nio.ByteBuffer;

/** One browser tab: a WebView in a host view over the GPUI surface, placed and clipped by Rust. */
final class ZzBrowser {
    static final int SNAPSHOT_PNG = 0;
    static final int SNAPSHOT_PIXELS = 1;

    private final long id;
    private final ZzActivity activity;
    private final Host host;
    private final WebView web;
    private final ProgressBar spinner;
    private final Handler main = new Handler(Looper.getMainLooper());
    private final float density;
    private boolean loading;
    private boolean refreshing;
    private boolean pulled;
    private float pullStart = Float.NaN;
    private boolean destroyed;

    static native boolean nativeAllow(long id, String url);

    static native void nativeChanged(
            long id, String url, String title, boolean loading, boolean back, boolean forward, boolean committed);

    static native void nativeError(long id, String message);

    static native void nativeOpen(long id, String url);

    static native void nativeFocused(long id);

    static native void nativeMessage(long id, String message);

    static native void nativeScrolled(long id, float travel, boolean atTop);

    static native void nativeRefresh(long id);

    static native void nativeTerminated(long id);

    static native void nativeSnapshot(long id, int kind, byte[] data, int width, int height);

    static ZzBrowser create(ZzActivity activity, long id, String profile) {
        return new ZzBrowser(activity, id, profile);
    }

    /** Sends every page through the host's SOCKS forward on {@code port}, or loads directly with -1. */
    static void route(int port) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE)) {
            return;
        }
        Runnable done = () -> {};
        if (port < 0) {
            ProxyController.getInstance().clearProxyOverride(Runnable::run, done);
            return;
        }
        ProxyConfig config = new ProxyConfig.Builder()
                .addProxyRule("socks://127.0.0.1:" + port)
                .addBypassRule("localhost")
                .addBypassRule("127.0.0.1")
                .addBypassRule("[::1]")
                .build();
        ProxyController.getInstance().setProxyOverride(config, Runnable::run, done);
    }

    @SuppressLint({"SetJavaScriptEnabled", "AddJavascriptInterface", "ClickableViewAccessibility"})
    private ZzBrowser(ZzActivity activity, long id, String profile) {
        this.id = id;
        this.activity = activity;
        density = activity.getResources().getDisplayMetrics().density;
        WebView.setWebContentsDebuggingEnabled(true);
        host = new Host(activity);
        host.setVisibility(View.GONE);
        web = new WebView(activity);
        if (WebViewFeature.isFeatureSupported(WebViewFeature.MULTI_PROFILE)) {
            ProfileStore.getInstance().getOrCreateProfile(profile);
            WebViewCompat.setProfile(web, profile);
        }
        WebSettings settings = web.getSettings();
        settings.setJavaScriptEnabled(true);
        settings.setDomStorageEnabled(true);
        settings.setMediaPlaybackRequiresUserGesture(false);
        settings.setSupportMultipleWindows(true);
        settings.setJavaScriptCanOpenWindowsAutomatically(true);
        settings.setBuiltInZoomControls(true);
        settings.setDisplayZoomControls(false);
        settings.setUseWideViewPort(true);
        settings.setLoadWithOverviewMode(true);
        web.addJavascriptInterface(new Picker(), "zzElementPicker");
        web.setWebViewClient(new Client());
        web.setWebChromeClient(new Chrome());
        web.setOnScrollChangeListener((view, x, y, oldX, oldY) -> {
            if (y != oldY) {
                nativeScrolled(id, (y - oldY) / density, y <= 0);
            }
        });
        web.setOnTouchListener((view, event) -> {
            track(event);
            return false;
        });
        host.addView(web, new FrameLayout.LayoutParams(
                FrameLayout.LayoutParams.MATCH_PARENT, FrameLayout.LayoutParams.MATCH_PARENT));
        spinner = new ProgressBar(activity);
        spinner.setIndeterminate(true);
        spinner.setVisibility(View.GONE);
        int size = Math.round(28 * density);
        FrameLayout.LayoutParams spot = new FrameLayout.LayoutParams(size, size, Gravity.CENTER_HORIZONTAL | Gravity.TOP);
        spot.topMargin = Math.round(14 * density);
        host.addView(spinner, spot);
        activity.root.addView(host, new FrameLayout.LayoutParams(0, 0));
    }

    private void track(MotionEvent event) {
        switch (event.getActionMasked()) {
            case MotionEvent.ACTION_DOWN:
                nativeFocused(id);
                pullStart = web.getScrollY() == 0 ? event.getY() : Float.NaN;
                pulled = false;
                break;
            case MotionEvent.ACTION_MOVE:
                if (!refreshing && !Float.isNaN(pullStart) && web.getScrollY() == 0) {
                    boolean past = event.getY() - pullStart >= 80 * density;
                    if (past != pulled) {
                        pulled = past;
                        spinner.setVisibility(past ? View.VISIBLE : View.GONE);
                    }
                }
                break;
            case MotionEvent.ACTION_UP:
                if (pulled && !refreshing) {
                    refreshing = true;
                    nativeRefresh(id);
                } else if (!refreshing) {
                    spinner.setVisibility(View.GONE);
                }
                pulled = false;
                pullStart = Float.NaN;
                break;
            case MotionEvent.ACTION_CANCEL:
                if (!refreshing) {
                    spinner.setVisibility(View.GONE);
                }
                pulled = false;
                pullStart = Float.NaN;
                break;
            default:
                break;
        }
    }

    private void changed(boolean committed) {
        if (destroyed) {
            return;
        }
        if (!loading && refreshing) {
            endRefresh();
        }
        nativeChanged(id, web.getUrl(), web.getTitle(), loading, web.canGoBack(), web.canGoForward(), committed);
    }

    void load(String url) {
        web.loadUrl(url);
    }

    void back() {
        if (web.canGoBack()) {
            web.goBack();
        }
    }

    void forward() {
        if (web.canGoForward()) {
            web.goForward();
        }
    }

    void reload() {
        web.reload();
    }

    void stop() {
        web.stopLoading();
    }

    void setVisible(boolean visible) {
        int visibility = visible ? View.VISIBLE : View.GONE;
        if (host.getVisibility() != visibility) {
            boolean focused = !visible && web.hasFocus();
            if (!visible) {
                web.clearFocus();
            }
            host.setVisibility(visibility);
            if (focused) {
                activity.reclaimFocus();
            }
        }
    }

    void setInteractive(boolean interactive) {
        host.interactive = interactive;
    }

    void endRefresh() {
        refreshing = false;
        spinner.setVisibility(View.GONE);
    }

    void mount(int x, int y, int width, int height, float radius, int holeX, int holeY, int holeWidth,
            int holeHeight, float holeRadius) {
        FrameLayout.LayoutParams params = (FrameLayout.LayoutParams) host.getLayoutParams();
        if (params.leftMargin != x || params.topMargin != y || params.width != width || params.height != height) {
            params.leftMargin = x;
            params.topMargin = y;
            params.width = width;
            params.height = height;
            host.setLayoutParams(params);
        }
        host.shape(width, height, radius, holeWidth > 0 && holeHeight > 0
                ? new RectF(holeX, holeY, holeX + holeWidth, holeY + holeHeight) : null, holeRadius);
    }

    void evaluate(String script) {
        web.evaluateJavascript(script, null);
    }

    void snapshot(int kind, int x, int y, int width, int height) {
        int[] origin = new int[2];
        web.getLocationInWindow(origin);
        Rect rect = new Rect(origin[0] + x, origin[1] + y, origin[0] + x + width, origin[1] + y + height);
        View decor = activity.getWindow().getDecorView();
        if (!rect.intersect(0, 0, decor.getWidth(), decor.getHeight()) || rect.isEmpty() || host.getVisibility() != View.VISIBLE) {
            nativeSnapshot(id, kind, null, 0, 0);
            return;
        }
        Bitmap bitmap = Bitmap.createBitmap(rect.width(), rect.height(), Bitmap.Config.ARGB_8888);
        PixelCopy.request(activity.getWindow(), rect, bitmap, result -> {
            if (destroyed || result != PixelCopy.SUCCESS) {
                bitmap.recycle();
                nativeSnapshot(id, kind, null, 0, 0);
                return;
            }
            byte[] data;
            if (kind == SNAPSHOT_PNG) {
                ByteArrayOutputStream out = new ByteArrayOutputStream();
                bitmap.compress(Bitmap.CompressFormat.PNG, 100, out);
                data = out.toByteArray();
            } else {
                ByteBuffer pixels = ByteBuffer.allocate(bitmap.getByteCount());
                bitmap.copyPixelsToBuffer(pixels);
                data = pixels.array();
            }
            int bitmapWidth = bitmap.getWidth();
            int bitmapHeight = bitmap.getHeight();
            bitmap.recycle();
            nativeSnapshot(id, kind, data, bitmapWidth, bitmapHeight);
        }, main);
    }

    void destroy() {
        destroyed = true;
        boolean focused = web.hasFocus();
        web.stopLoading();
        activity.root.removeView(host);
        host.removeAllViews();
        web.destroy();
        if (focused) {
            activity.reclaimFocus();
        }
    }

    private final class Picker {
        @JavascriptInterface
        public void postMessage(String message) {
            main.post(() -> {
                if (!destroyed) {
                    nativeMessage(id, message);
                }
            });
        }
    }

    private final class Client extends WebViewClient {
        @Override
        public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
            return !nativeAllow(id, request.getUrl().toString());
        }

        @Override
        public void onPageStarted(WebView view, String url, Bitmap favicon) {
            loading = true;
            changed(false);
        }

        @Override
        public void onPageCommitVisible(WebView view, String url) {
            changed(true);
        }

        @Override
        public void onPageFinished(WebView view, String url) {
            loading = false;
            changed(true);
        }

        @Override
        public void doUpdateVisitedHistory(WebView view, String url, boolean reload) {
            changed(false);
        }

        @Override
        public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
            if (request.isForMainFrame() && error.getErrorCode() != ERROR_UNKNOWN) {
                nativeError(id, error.getDescription().toString());
            }
        }

        @Override
        public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
            nativeTerminated(id);
            return true;
        }
    }

    private final class Chrome extends WebChromeClient {
        @Override
        public void onReceivedTitle(WebView view, String title) {
            changed(false);
        }

        @Override
        public void onProgressChanged(WebView view, int progress) {
            boolean busy = progress < 100;
            if (busy != loading) {
                loading = busy;
                changed(false);
            }
        }

        @Override
        public boolean onCreateWindow(WebView view, boolean dialog, boolean gesture, Message result) {
            WebView probe = new WebView(view.getContext());
            probe.setWebViewClient(new WebViewClient() {
                @Override
                public boolean shouldOverrideUrlLoading(WebView child, WebResourceRequest request) {
                    nativeOpen(id, request.getUrl().toString());
                    child.destroy();
                    return true;
                }
            });
            WebView.WebViewTransport transport = (WebView.WebViewTransport) result.obj;
            transport.setWebView(probe);
            result.sendToTarget();
            return true;
        }
    }

    /** Clips the tab to rounded lower corners and leaves a hole where GPUI chrome floats over it. */
    static final class Host extends FrameLayout {
        private final Path clip = new Path();
        private final RectF hole = new RectF();
        boolean interactive = true;

        Host(Context context) {
            super(context);
        }

        void shape(int width, int height, float radius, RectF cutout, float cutoutRadius) {
            clip.reset();
            clip.setFillType(Path.FillType.EVEN_ODD);
            clip.addRoundRect(new RectF(0, 0, width, height),
                    new float[] {0, 0, 0, 0, radius, radius, radius, radius}, Path.Direction.CW);
            if (cutout != null) {
                hole.set(cutout);
                float round = Math.min(cutoutRadius, cutout.height() / 2);
                clip.addRoundRect(cutout, round, round, Path.Direction.CW);
            } else {
                hole.setEmpty();
            }
            invalidate();
        }

        @Override
        protected void dispatchDraw(Canvas canvas) {
            int saved = canvas.save();
            if (!clip.isEmpty()) {
                canvas.clipPath(clip);
            }
            super.dispatchDraw(canvas);
            canvas.restoreToCount(saved);
        }

        @Override
        public boolean dispatchTouchEvent(MotionEvent event) {
            if (!interactive) {
                return false;
            }
            if (event.getActionMasked() == MotionEvent.ACTION_DOWN && hole.contains(event.getX(), event.getY())) {
                return false;
            }
            return super.dispatchTouchEvent(event);
        }
    }
}
