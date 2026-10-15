flake:
{
  lib,
  pkgs,
  config,
  ...
}:
let
  cfg = config.services.rat-polkit;
in
{
  options.services.rat-polkit = {
    enable = lib.mkEnableOption "rat-polkit";
    package = lib.mkOption {
      type = lib.types.package;
      default = flake.packages.${pkgs.stdenv.system}.rat-polkit;
      defaultText = lib.literalExpression "flake.packages.\${pkgs.stdenv.system}.rat-polkit";
      description = "The rat-polkit package to use.";
    };
    terminal = {
      bin = lib.mkOption {
        type = lib.types.str;
        default = lib.getExe' pkgs.kitty "kitten";
        description = "Terminal that gets spawned when polkit sends an auth request.";
      };
      args = lib.mkOption {
        type = lib.types.str;
        default = "panel --layer top --edge center-sized --focus-policy on-demand";
        description = "Args to pass to terminal.";
      };

      width = lib.mkOption {
        type = lib.types.int;
        default = 40;
        description = "Width of the dialog box.";
      };

      height = lib.mkOption {
        type = lib.types.int;
        default = 20;
        description = "Height of the dialog box.";
      };
    };
  };
  config = lib.mkIf cfg.enable {
    systemd.user.services.rat-polkit-authentication-agent-1 = {
      Unit = {
        Description = "rat-polkit-authentication-agent-1";
        After = [ "graphical-session.target" ];
        Wants = [ "graphical-session.target" ];
      };
      Install = {
        WantedBy = [ "graphical-session.target" ];
      };
      Service = {
        Type = "simple";
        ExecStart = "${lib.getExe pkgs.bash} -c '${lib.getExe cfg.package} handler --bin ${lib.getExe cfg.package} --terminal-bin ${cfg.terminal.bin} --terminal-args=\"${cfg.terminal.args} --lines ${toString cfg.terminal.height} --columns ${toString cfg.terminal.width}\"'";
        Restart = "on-failure";
        RestartSec = 1;
        TimeoutStopSec = 10;
      };
    };
  };
}
