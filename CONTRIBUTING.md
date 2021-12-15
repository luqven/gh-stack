# Contributing

This project is maintained by @luqven. If you have any questions or suggestions, please open an issue or a pull request.

# Release

To release a new version of the project, you need to:

1. Update the version number in the `Cargo.toml` file
   ```toml
    [package]
    version = "0.1.0" // <-- Update this to next version
    // ...
   ```
2. Create a release commit with the changes and tag it with the new version number
   ```bash
   git commit -m "chore: release v0.1.0" && git tag v0.1.0
   ```
3. Push the commit and tag to the remote repository
   ```bash
   git push origin master && git push origin master --tags
   ```
4. Create `target/release/` folder with the binary
   ```bash
   cargo build --release
   ```
5. Create a `.tar.gz` archive of the release bundle that's compatible with Brew and GitHub Releases.
   ```bash
   cd target/release
   tar -czf gh-stack-mac.tar.gz gh-stack
   ```
6. Upload the release bundle to the `luqven/gh-stack` repository on GitHub Releases.

   Navigate to the Releases section and then click on "Create a new release".
   Insert a tag version, IE `v0.1.0`, a title, and then drag the previously created `.tar.gz` archive into the upload section. Click on the `Publish release` button.

7. Copy the sha256sum of the archive to the `homebrew-gh-stack/Formula/gh-stack.rb` file and update the version number.

   ```bash
   cd target/release
   shasum -a 256 gh-stack-mac.tar.gz
   ```

   ```ruby
   # homebrew-gh-stack repository
   # /Formula/gh-stack.rb
   class GhStack < Formula
      desc "Cross-platform Text Expander written in Rust"
      homepage "https://github.com/luqven/gh-stack"
      version "0.1.0"
      url "https://github.com/luqven/gh-stack/releases/download/#{version}/gh-stack-mac.tar.gz"
      sha256 "<copied_sha>"
      ...
   end
   ```
