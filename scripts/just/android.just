import "settings.just"

set working-directory := "../.."

# Install the Android SDK, NDK, emulator and system image, the Rust targets, and the zz-phone AVD.
setup:
    @scripts/android.sh setup

# Boot the zz-phone emulator if none is running.
emulator:
    @scripts/android.sh emulator

# Build, install and launch on the emulator, attached to `just android rig` when it is up.
run:
    @scripts/android.sh run

# Build the emulator APK without installing it.
build:
    @scripts/android.sh build

# Build a release arm64 APK, install it on a USB or wireless device, and launch it.
device:
    @scripts/android.sh device

# Follow the running app's log.
logs:
    @scripts/android.sh logs

# Start the throwaway daemon the iOS rig uses, or stop it; `run` reaches it through adb reverse.
rig action="start":
    @scripts/ios-rig.sh {{ action }}
