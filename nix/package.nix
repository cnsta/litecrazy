{
  lib,
  udev,
  pkg-config,
  rustPlatform,
  dbus,
  makeWrapper,
  rev ? "dirty",
}: let
  cargoToml = lib.importTOML ../Cargo.toml;
  runtimeDeps = [
    udev
    dbus
  ];
in
  rustPlatform.buildRustPackage {
    pname = "litecrazy";
    version = "${cargoToml.package.version}-${rev}";

    src = lib.fileset.toSource {
      root = ../.;
      fileset = lib.fileset.unions [
        ../src
        ../assets
        ../build.rs
        ../Cargo.lock
        ../Cargo.toml
      ];
    };

    cargoLock.lockFile = ../Cargo.lock;
    strictDeps = true;

    nativeBuildInputs = [
      pkg-config
      rustPlatform.bindgenHook
      makeWrapper
    ];

    buildInputs = runtimeDeps;

    postInstall = ''
      for bin in $out/bin/*; do
        wrapProgram $bin \
          --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath runtimeDeps}"
      done
    '';

    meta = {
      description = "Battery tray icon for the Pulsar X2 CrazyLight gaming mouse";
      longDescription = ''
        LiteCrazy puts the battery level of a Pulsar X2 CrazyLight in the
        system tray, warns when it runs low, and opens the Pulsar web
        configurator in a Chromium-based browser — the configurator drives the
        mouse over WebHID, so device settings are made there rather than here.
      '';
      homepage = "https://github.com/cnsta/litecrazy";
      license = lib.licenses.mit;
      mainProgram = "litecrazy";
      platforms = lib.platforms.linux;
    };
  }
