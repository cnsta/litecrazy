self: {
  config,
  pkgs,
  lib,
  ...
}: let
  inherit (lib.options) mkEnableOption mkOption mkPackageOption;
  inherit (lib) mkIf mkMerge types optional;
  cfg = config.hardware.litecrazy;

  # `browser` accepts either a package (the common case, `pkgs.chromium`) or
  # a bare string for something not in nixpkgs.
  browserBin =
    if cfg.service.browser == null
    then null
    else if lib.isDerivation cfg.service.browser
    then lib.getExe cfg.service.browser
    else toString cfg.service.browser;
in {
  options.hardware.litecrazy = {
    enable = mkEnableOption "LiteCrazy — installs the package and udev rules for the Pulsar X2 CrazyLight";

    package =
      mkPackageOption pkgs "litecrazy" {}
      // {
        default = self.packages.${pkgs.stdenv.hostPlatform.system}.litecrazy;
      };

    service = {
      enable = mkEnableOption ''
        Systemd user service.

        Runs `litecrazy` in tray mode as a systemd user service, started
        automatically with your graphical session.

        Requires `hardware.litecrazy.enable = true`.
      '';

      url = mkOption {
        type = types.str;
        default = "https://bbb.pulsar.gg/";
        description = "URL opened by the tray's Open Configurator item.";
      };

      browser = mkOption {
        type = types.nullOr (types.either types.package types.str);
        default = null;
        example = lib.literalExpression "pkgs.chromium";
        description = ''
          Browser used for the configurator. When null, litecrazy searches
          for a Chromium-based browser itself.

          The configurator drives the mouse over WebHID, which only
          Chromium-derived browsers implement — Firefox will load the page
          but never see the device.
        '';
      };

      windowMode = mkOption {
        type = types.enum [
          "app"
          "tab"
        ];
        default = "app";
        description = ''
          "app" opens the configurator as a chrome-less window with
          class "litecrazy"; "tab" opens it as an ordinary browser tab.
        '';
      };

      browserArgs = mkOption {
        type = types.listOf types.str;
        default = [];
        example = ["--ozone-platform=wayland"];
        description = "Extra flags appended to the browser invocation.";
      };

      batteryInterval = mkOption {
        type = types.ints.between 10 3600;
        default = 60;
        description = "Seconds between battery polls.";
      };

      lowBatteryThreshold = mkOption {
        type = types.ints.between 0 100;
        default = 20;
        description = "Battery percentage that triggers a notification. 0 disables them.";
      };

      pauseMinutes = mkOption {
        type = types.ints.between 0 240;
        default = 10;
        description = ''
          How long battery polling is suspended after opening the
          configurator, so the tray and the web page don't drive the same
          hidraw node at once. 0 disables the auto-pause.
        '';
      };
    };
  };

  config = mkIf cfg.enable (mkMerge [
    {
      environment.systemPackages = [cfg.package];

      # Grant the logged-in user access to the HID interface without root.
      # TAG+="uaccess" makes logind grant seat-local access automatically.
      #
      # These same rules are what lets Chromium's WebHID reach the mouse from
      # the configurator page — the browser opens the hidraw node as your user.
      services.udev.extraRules = ''
        # Pulsar X2 CrazyLight (wired)
        SUBSYSTEM=="usb",  ATTRS{idVendor}=="3710", ATTRS{idProduct}=="3414", TAG+="uaccess"
        # Pulsar 8K Dongle (wireless)
        SUBSYSTEM=="usb",  ATTRS{idVendor}=="3710", ATTRS{idProduct}=="5406", TAG+="uaccess"
        # hidraw nodes — required for non-root HID access via hidapi and WebHID
        KERNEL=="hidraw*", ATTRS{idVendor}=="3710", ATTRS{idProduct}=="3414", TAG+="uaccess"
        KERNEL=="hidraw*", ATTRS{idVendor}=="3710", ATTRS{idProduct}=="5406", TAG+="uaccess"
        # Interface 0, presented as a mouse; suppress keyboard classification
        SUBSYSTEM=="input", ATTRS{idVendor}=="3710", ATTRS{idProduct}=="5406", ATTRS{bInterfaceNumber}=="00", ENV{ID_INPUT_KEYBOARD}="0", ENV{ID_INPUT_KEY}="0"
        # Interface 2, presented as a keyboard/consumer control; suppress entirely
        SUBSYSTEM=="input", ATTRS{idVendor}=="3710", ATTRS{idProduct}=="5406", ATTRS{bInterfaceNumber}=="02", ENV{ID_INPUT_KEYBOARD}="0", ENV{ID_INPUT_KEY}="0", ENV{ID_INPUT_MOUSE}="0"
      '';
    }

    (mkIf cfg.service.enable {
      systemd.user.services.litecrazy = {
        description = "litecrazy battery tray";
        wantedBy = ["graphical-session.target"];
        partOf = ["graphical-session.target"];
        after = ["graphical-session.target"];

        serviceConfig = {
          Type = "simple";
          ExecStart = "${cfg.package}/bin/litecrazy";
          Restart = "on-failure";
          RestartSec = "5s";

          # A user unit does not inherit the login shell's PATH, so without
          # this the browser lookup would only ever find system-wide packages.
          # litecrazy also probes these directories itself, belt and braces.
          Environment =
            [
              "PATH=/etc/profiles/per-user/%u/bin:/run/current-system/sw/bin:/nix/var/nix/profiles/default/bin"
              "LITECRAZY_URL=${cfg.service.url}"
              "LITECRAZY_INTERVAL=${toString cfg.service.batteryInterval}"
              "LITECRAZY_LOW_THRESHOLD=${toString cfg.service.lowBatteryThreshold}"
              "LITECRAZY_PAUSE_MINUTES=${toString cfg.service.pauseMinutes}"
              "LITECRAZY_WINDOW_MODE=${cfg.service.windowMode}"
            ]
            ++ optional (browserBin != null) "LITECRAZY_BROWSER=${browserBin}"
            ++ optional (cfg.service.browserArgs != []) "LITECRAZY_BROWSER_ARGS=${
              lib.concatStringsSep " " cfg.service.browserArgs
            }";

          PassEnvironment = [
            "WAYLAND_DISPLAY"
            "DISPLAY"
            "XAUTHORITY"
            "XDG_RUNTIME_DIR"
            "XDG_CURRENT_DESKTOP"
            "XDG_SESSION_TYPE"
            "DBUS_SESSION_BUS_ADDRESS"
          ];
        };
      };
    })
  ]);
}
