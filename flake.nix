{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs =
    {
      self,
      nixpkgs,
      ...
    }:
    let
      inherit (nixpkgs) lib;
      systems = lib.systems.flakeExposed;
      eachSystem =
        f: lib.genAttrs systems (system: f (nixpkgs.legacyPackages.${system}.extend self.overlays.default));
    in
    {
      packages = eachSystem (pkgs: {
        default = pkgs.rat-polkit;
        rat-polkit = pkgs.rat-polkit;
      });
      devShells = eachSystem (pkgs: {
        default = pkgs.mkShell {
          name = "rat-polkit";
          inputsFrom = [ pkgs.rat-polkit ];
          buildInputs = [
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.rustfmt
          ];
        };
      });
      overlays.default = final: prev: {
        rat-polkit = final.callPackage ./package.nix { };
      };
    };
}
