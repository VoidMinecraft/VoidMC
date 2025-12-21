import * as path from "node:path";
import { defineConfig } from "rspress/config";

export default defineConfig({
  root: path.join(__dirname, "docs"),
  title: "My Site",
  icon: "/rspress-icon.png",
  logo: {
    light: "/rspress-light-logo.png",
    dark: "/rspress-dark-logo.png",
  },
  description: "A performant, modular Minecraft server implementation in Rust",
  themeConfig: {
    socialLinks: [
      {
        icon: "github",
        mode: "link",
        content:
          "https://github.com/EpitechPromo2027/G-EIP-600-lil-6-1-eip-adam.cavillon",
      },
    ],
  },
});
