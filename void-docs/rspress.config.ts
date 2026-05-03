import * as path from "node:path";
import { defineConfig } from "rspress/config";

export default defineConfig({
  root: path.join(__dirname, "docs"),
  base: "/VoidMC/",
  title: "VoidMC",
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
          "https://github.com/VoidMinecraft/VoidMC",
      },
    ],
  },
});
