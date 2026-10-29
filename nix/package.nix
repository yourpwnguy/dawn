{ lib, rustPlatform, pkg-config, wayland, wayland-scanner, libxkbcommon
, libx11, libxcb, vulkan-loader, libGL, fontconfig }:

let
  manifest = lib.importTOML ../Cargo.toml;
  source = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      let
        root = toString ../.;
        relative = lib.removePrefix (root + "/") (toString path);
      in
        !(lib.hasPrefix "target" relative)
        && !(lib.hasPrefix ".git" relative)
        && !(lib.hasPrefix "docs" relative)
        && !(lib.hasPrefix "examples/themes" relative)
        && (
          !(lib.hasPrefix "examples/" relative)
          || type == "directory"
          || lib.hasSuffix "/examples/wallpapers/input.mp4" (toString path)
        );
  };
in
rustPlatform.buildRustPackage {
  pname = "dawn";
  version = manifest.package.version;

  inherit src;

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ pkg-config wayland-scanner ];

  buildInputs = [
    wayland
    libxkbcommon
    libx11
    libxcb
    vulkan-loader
    libGL
    fontconfig
  ];

  meta = {
    description = manifest.package.description;
    license = with lib.licenses; [ mit asl20 ];
    mainProgram = "dawn";
    platforms = lib.platforms.linux;
  };
}
