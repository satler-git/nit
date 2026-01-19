{ config, lib, pkgs, ... }:

with lib;

let
  cfg = config.programs.nit;
  configFormat = pkgs.formats.toml { };
in
{
  options.programs.nit = {
    enable = mkEnableOption "nit - a Nix flake template launcher";

    package = mkPackageOption pkgs "nit" { };

    settings = mkOption {
      type = types.submodule {
        options = {
          template = mkOption {
            description = "List of template sources";
            type = types.listOf (types.submodule {
              options = {
                name = mkOption {
                  type = types.nullOr types.str;
                  default = null;
                  description = "Optional name for the template collection";
                };
                uri = mkOption {
                  type = types.str;
                  description = "Flake URI for templates (e.g., github:NixOS/templates)";
                };
                templates = mkOption {
                  type = types.nullOr (types.listOf types.str);
                  default = null;
                  description = "List of specific templates to include. If omitted, imports all templates";
                };
                execludes = mkOption {
                  type = types.nullOr (types.listOf types.str);
                  default = null;
                  description = "List of templates to exclude";
                };
              };
            });
            default = [ ];
            example = literalExpression ''
              [
                {
                  uri = "github:NixOS/templates";
                  templates = [ "default" ];
                }
              ]
            '';
          };
        };
      };
      default = { template = [ ]; };
      description = "Configuration for nit";
    };
  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ];

    xdg.configFile."nix-nit/config.toml" = {
      source = configFormat.generate "nit-config.toml" cfg.settings;
    };
  };
}
