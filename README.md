# Yuri Rewrite

Yuri Rewrite 是一个 Windows 本地桌面工具，用于导入 TXT 小说、按章节分析大纲/人物/关系/地点/伏笔，并通过用户配置的 AI 模型逐章改写为双女主百合文本。

## 当前 MVP

- 本地 TXT 导入，支持 UTF-8 和 GBK 常见编码。
- 自动章节识别；无法识别章节时按长度自动分段。
- 本地 SQLite 保存小说、章节、分析结果、改写稿、任务状态和一致性资产。
- OpenAI-compatible 模型接口，适配 ChatGPT、Moonshot、MiniMax、智谱、百炼等兼容端点。
- Gemini 单独适配。
- 人物卡、人物关系、地点、伏笔、术语表可编辑。
- 原文/改写稿左右对照。
- 导出 TXT 或 Markdown。

## 开发

```powershell
npm install
npm run tauri:dev
```

## 构建

```powershell
npm run tauri:build
npm run package:portable
```

便携版会生成在 `portable/YuriRewrite-vX.Y.Z-windows-x64.zip`。

## 给其他 Windows 用户使用

1. 将 zip 发给用户。
2. 用户解压到任意目录。
3. 双击 `yuri-rewrite.exe` 启动。
4. 首次使用时在软件内填写模型 Base URL、模型名和 API Key。

第一版暂不签名，Windows SmartScreen 可能提示未知发布者。应用数据保存在当前 Windows 用户的应用数据目录中，替换程序目录不会删除小说和配置。

## OpenAI-compatible 配置示例

- OpenAI: `https://api.openai.com/v1`
- Moonshot: `https://api.moonshot.cn/v1`
- 百炼 DashScope 兼容模式: 按百炼控制台提供的 OpenAI-compatible Base URL 填写。
- 智谱、MiniMax: 使用其控制台提供的 OpenAI-compatible Base URL。

模型名请填写供应商当前提供的具体模型 ID。
