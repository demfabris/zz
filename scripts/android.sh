#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"
mode="${1:-run}"
[[ "$mode" =~ ^(setup|emulator|run|build|device|logs)$ ]] || {
    echo "expected setup, emulator, run, build, device, or logs" >&2
    exit 2
}

sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}"
avd="${ZZ_ANDROID_AVD:-zz-phone}"
ndk_version=29.0.14206865
platform_version=36
build_tools_version=36.1.0
system_image="system-images;android-36;google_apis;x86_64"
min_sdk=30
host_tag="$([[ "$(uname -s)" == Darwin ]] && echo darwin-x86_64 || echo linux-x86_64)"

if [[ -z "${JAVA_HOME:-}" && -x "$HOME/.local/share/jdk/21/bin/java" ]]; then
    export JAVA_HOME="$HOME/.local/share/jdk/21"
fi
[[ -n "${JAVA_HOME:-}" ]] && export PATH="$JAVA_HOME/bin:$PATH"
adb="$sdk/platform-tools/adb"
emulator_bin="$sdk/emulator/emulator"
sdkmanager="$sdk/cmdline-tools/latest/bin/sdkmanager"
avdmanager="$sdk/cmdline-tools/latest/bin/avdmanager"

if [[ "$mode" == setup ]]; then
    command -v java >/dev/null || { echo "install JDK 17 or newer first (or unpack one at ~/.local/share/jdk/21)" >&2; exit 1; }
    if [[ ! -x "$sdkmanager" ]]; then
        echo "==> installing the Android command-line tools into $sdk"
        tools_zip="$(mktemp -d)/tools.zip"
        repository="https://dl.google.com/android/repository"
        tools_os="$([[ "$host_tag" == darwin-x86_64 ]] && echo mac || echo linux)"
        latest="$(curl -fsSL "$repository/repository2-3.xml" | grep -o "commandlinetools-$tools_os-[0-9]*_latest.zip" | sort -t- -k3 -n | tail -1)"
        curl -fsSL "$repository/$latest" -o "$tools_zip"
        mkdir -p "$sdk/cmdline-tools"
        rm -rf "$sdk/cmdline-tools/latest" "$sdk/cmdline-tools/cmdline-tools"
        unzip -q "$tools_zip" -d "$sdk/cmdline-tools"
        mv "$sdk/cmdline-tools/cmdline-tools" "$sdk/cmdline-tools/latest"
    fi
    yes | "$sdkmanager" --licenses >/dev/null
    "$sdkmanager" --install "platform-tools" "build-tools;$build_tools_version" "platforms;android-$platform_version" \
        "ndk;$ndk_version" "emulator" "$system_image"
    rustup target add x86_64-linux-android aarch64-linux-android
    if ! "$emulator_bin" -list-avds | grep -qx "$avd"; then
        echo no | "$avdmanager" create avd -n "$avd" -k "$system_image" -d pixel_8
    fi
    config="$HOME/.android/avd/$avd.avd/config.ini"
    if [[ -f "$config" ]]; then
        sed -i.bak -e 's/^hw.keyboard=.*/hw.keyboard=no/' "$config"
        grep -q '^hw.keyboard=' "$config" || echo 'hw.keyboard=no' >> "$config"
    fi
    echo "Android SDK ready in $sdk; just android emulator boots $avd"
    exit 0
fi

[[ -d "$sdk" ]] || { echo "no Android SDK at $sdk; run just android setup" >&2; exit 2; }

device_serial() {
    "$adb" devices | awk 'NR > 1 && $2 == "device" { print $1 }' | grep -E "${1:-.}" | head -1
}

boot_emulator() {
    if [[ -n "$(device_serial '^emulator-')" ]]; then
        return
    fi
    local args=(-avd "$avd" -no-snapshot-save -no-boot-anim -no-audio)
    if [[ -n "${ZZ_ANDROID_HEADLESS:-}" ]]; then
        args+=(-no-window)
    fi
    if [[ -z "${DISPLAY:-}${WAYLAND_DISPLAY:-}" && "$host_tag" == linux-x86_64 ]]; then
        args+=(-no-window -gpu swiftshader_indirect)
    fi
    echo "==> booting $avd"
    mkdir -p "$repo_root/target/android"
    nohup "$emulator_bin" "${args[@]}" > "$repo_root/target/android/emulator.log" 2>&1 &
    "$adb" wait-for-device
    until [[ "$("$adb" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" == 1 ]]; do sleep 1; done
    "$adb" shell settings put secure show_ime_with_hard_keyboard 1
}

if [[ "$mode" == emulator ]]; then
    boot_emulator
    echo "emulator $(device_serial '^emulator-') is up"
    exit 0
fi

dev_build="${ZZ_DEV_BUILD-1}"
if [[ "$dev_build" == 1 ]]; then
    package=dev.zz.android.dev label="zz Dev" scheme=zz-dev icon=zz-dev
else
    package=dev.zz.android label=zz scheme=zz icon=zz
fi

if [[ "$mode" == logs ]]; then
    pid="$("$adb" shell pidof "$package" | tr -d '\r')"
    [[ -n "$pid" ]] || { echo "$package is not running" >&2; exit 1; }
    exec "$adb" logcat --pid="$pid"
fi

if [[ "$mode" == device ]]; then
    target=aarch64-linux-android abi=arm64-v8a
    cargo_profile="${ZZ_ANDROID_CARGO_PROFILE:-release}"
else
    target="${ZZ_ANDROID_TARGET:-x86_64-linux-android}"
    abi="$([[ "$target" == aarch64-linux-android ]] && echo arm64-v8a || echo x86_64)"
    cargo_profile="${ZZ_ANDROID_CARGO_PROFILE:-dev}"
fi
profile_dir="$([[ "$cargo_profile" == dev ]] && echo debug || echo "$cargo_profile")"

ndk="${ANDROID_NDK_HOME:-$sdk/ndk/$ndk_version}"
toolchain="$ndk/toolchains/llvm/prebuilt/$host_tag/bin"
build_tools="$sdk/build-tools/$build_tools_version"
android_jar="$sdk/platforms/android-$platform_version/android.jar"
for required in "$toolchain" "$build_tools" "$android_jar"; do
    [[ -e "$required" ]] || { echo "missing $required; run just android setup" >&2; exit 2; }
done

triple_env="$(echo "$target" | tr '-' '_')"
target_env="$(echo "$target" | tr 'a-z-' 'A-Z_')"
export "CC_$triple_env=$toolchain/$target$min_sdk-clang"
export "CXX_$triple_env=$toolchain/$target$min_sdk-clang++"
export "AR_$triple_env=$toolchain/llvm-ar"
export "CARGO_TARGET_${target_env}_LINKER=$toolchain/$target$min_sdk-clang"

ZZ_DEV_BUILD="$dev_build" cargo build --locked --profile "$cargo_profile" -p zz-android --target "$target"

out="$repo_root/target/android/$abi"
rm -rf "$out/stage" "$out/classes" "$out/dex" "$out/apk"
mkdir -p "$out/stage/res/mipmap-xxxhdpi" "$out/classes" "$out/dex" "$out/apk/lib/$abi"
cp -R "$repo_root/clients/android/res/." "$out/stage/res/"
cp "$repo_root/assets/linux/hicolor/256x256/apps/$icon.png" "$out/stage/res/mipmap-xxxhdpi/ic_launcher.png"
workspace_version="$(sed -nE 's/^version = "([^"]+)"$/\1/p' "$repo_root/Cargo.toml" | head -1)"
version_code="${ZZ_ANDROID_VERSION_CODE:-$(( $(date -u +%s) / 60 - 29000000 ))}"
sed -e "s/@PACKAGE@/$package/g" -e "s/@LABEL@/$label/g" -e "s/@SCHEME@/$scheme/g" \
    -e "s/@VERSION_CODE@/$version_code/g" -e "s/@VERSION_NAME@/$workspace_version/g" \
    "$repo_root/clients/android/AndroidManifest.xml" > "$out/stage/AndroidManifest.xml"

webkit_version=1.14.0
webkit_sha256=265604786ea1c9679e3f860f7f13072375c141f46db9092ed8ef5ecd599a516d
webkit_jar="$repo_root/target/android/deps/webkit-$webkit_version.jar"
if [[ ! -f "$webkit_jar" ]]; then
    mkdir -p "$(dirname "$webkit_jar")"
    webkit_aar="$(mktemp -d)/webkit.aar"
    curl -fsSL "https://dl.google.com/dl/android/maven2/androidx/webkit/webkit/$webkit_version/webkit-$webkit_version.aar" -o "$webkit_aar"
    [[ "$(shasum -a 256 "$webkit_aar" | cut -d' ' -f1)" == "$webkit_sha256" ]] \
        || { echo "androidx.webkit $webkit_version download does not match its checksum" >&2; exit 1; }
    unzip -p "$webkit_aar" classes.jar > "$webkit_jar"
fi

debug_mode=()
[[ "$cargo_profile" == dev || "$dev_build" == 1 ]] && debug_mode=(--debug-mode)
"$build_tools/aapt2" compile --dir "$out/stage/res" -o "$out/res.zip"
"$build_tools/aapt2" link -o "$out/base.apk" -I "$android_jar" --manifest "$out/stage/AndroidManifest.xml" \
    --min-sdk-version "$min_sdk" --target-sdk-version "$platform_version" "${debug_mode[@]}" "$out/res.zip"
javac --release 11 -nowarn -classpath "$android_jar:$webkit_jar" -d "$out/classes" $(find "$repo_root/clients/android/java" -name '*.java')
"$build_tools/d8" --min-api "$min_sdk" --lib "$android_jar" --output "$out/dex" \
    $(find "$out/classes" -name '*.class') "$webkit_jar" 2> "$out/d8.log" \
    || { cat "$out/d8.log" >&2; exit 1; }
"$toolchain/llvm-strip" --strip-debug -o "$out/apk/lib/$abi/libzz_android.so" "$repo_root/target/$target/$profile_dir/libzz_android.so"
cp "$out/dex/classes.dex" "$out/apk/classes.dex"
cp "$out/base.apk" "$out/unaligned.apk"
(cd "$out/apk" && zip -q -X "$out/unaligned.apk" classes.dex && zip -q -X -0 -r "$out/unaligned.apk" lib)
"$build_tools/zipalign" -P 16 -f 4 "$out/unaligned.apk" "$out/zz.apk"
keystore="${ZZ_ANDROID_KEYSTORE:-$HOME/.android/debug.keystore}"
if [[ ! -f "$keystore" ]]; then
    mkdir -p "$(dirname "$keystore")"
    keytool -genkeypair -keystore "$keystore" -storepass android -keypass android -alias androiddebugkey \
        -keyalg RSA -keysize 2048 -validity 10000 -dname "CN=Android Debug,O=Android,C=US" >/dev/null
fi
"$build_tools/apksigner" sign --ks "$keystore" --ks-pass "pass:${ZZ_ANDROID_KEYSTORE_PASS:-android}" "$out/zz.apk"
echo "==> built $out/zz.apk ($package)"
[[ "$mode" == build ]] && exit 0

if [[ "$mode" == device ]]; then
    serial="${ZZ_ANDROID_DEVICE:-$(device_serial '^[^e]')}"
    [[ -n "$serial" ]] || { echo "no USB or wireless Android device; enable USB debugging and run adb devices" >&2; exit 1; }
else
    boot_emulator
    serial="${ZZ_ANDROID_DEVICE:-$(device_serial '^emulator-')}"
fi
"$adb" -s "$serial" install -r "$out/zz.apk"
extras=()
for key in ZZ_GPUI_ENDPOINT ZZ_GPUI_SESSION ZZ_GPUI_FRAME_LOG ZZ_GPUI_BENCH ZZ_GPUI_GLASS GPUI_FRAME_STATS RUST_LOG; do
    [[ -n "${!key:-}" ]] && extras+=(--es "$key" "${!key}")
done
if [[ "$mode" == run && -z "${ZZ_GPUI_ENDPOINT:-}" ]]; then
    rig_socket="${ZZ_IOS_RIG_SOCKET:-/tmp/zzios-$(basename "$repo_root").sock}"
    if [[ -S "$rig_socket" ]]; then
        "$adb" -s "$serial" reverse localabstract:zz-rig "localfilesystem:$rig_socket" >/dev/null
        extras+=(--es ZZ_GPUI_ENDPOINT @zz-rig)
    fi
fi
"$adb" -s "$serial" shell am force-stop "$package"
"$adb" -s "$serial" shell am start -W -n "$package/dev.zz.android.ZzActivity" "${extras[@]}" >/dev/null
echo "==> launched $package on $serial; just android logs follows its log"
