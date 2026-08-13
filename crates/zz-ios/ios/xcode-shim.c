// Placates Xcode: an app target must compile and link *something* before its
// build phases run. The post-build script replaces the resulting executable
// with the cargo-built `zz-ios` binary, and Xcode signs that.
int main(void) { return 0; }
