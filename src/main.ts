import { createApp } from "vue";
import "./tauri-mock"; // 浏览器预览时模拟 Tauri API；真实环境自动跳过
import App from "./App.vue";

createApp(App).mount("#app");
