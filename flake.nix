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
      eachSystem = f: lib.genAttrs systems (system: f (nixpkgs.legacyPackages.${system}));
    in
    {
      packages = eachSystem (pkgs: {
        default = self.packages.${pkgs.stdenv.system}.rat-polkit;
        rat-polkit = pkgs.callPackage ./package.nix { };
      });
      devShells = eachSystem (pkgs: {
        default = pkgs.mkShell {
          name = "rat-polkit";
          inputsFrom = [ self.packages.${pkgs.stdenv.system}.rat-polkit ];
          buildInputs = [
            pkgs.rust-analyzer
            pkgs.clippy
            pkgs.rustfmt
          ];
        };
      });
      homeModules = {
        default = import ./module.nix self;
      };
      overlays.rat-polkit = final: prev: {
        rat-polkit = self.packages.${final.stdenv.system}.rat-polkit;
      };
      overlays.default = self.overlays.rat-polkit;
    };
}
