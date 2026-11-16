# macOS builds

Release `.dmg` files are built on GitHub's macOS runners (see `.github/workflows/release.yml`).

## Building the .dmg in Docker (`make mac`)

Building for macOS on Linux needs Apple's macOS SDK. Apple's license only permits using the SDK on Apple hardware, so this repository neither includes nor downloads it. If you have a Mac with Xcode, you can package your own SDK:

1. On the Mac, with Xcode installed, package the SDK with osxcross's tool:

   ```
   git clone https://github.com/tpoechtrager/osxcross
   cd osxcross
   ./tools/gen_sdk_package.sh
   ```

   This writes a file such as `MacOSX15.0.sdk.tar.xz`.

2. Copy that file into `packaging/macos/sdk/` in this repository. That folder is ignored by git.

3. Run `make mac`. The first run builds the `fastumpl-mac` image, which compiles osxcross against your SDK and takes a while. The result is `dist/fastumpl-macos-universal.dmg`.

The Docker-built image is a hybrid ISO/HFS disk image that macOS mounts like any `.dmg`. The app is unsigned: open it the first time with right-click, then Open.
