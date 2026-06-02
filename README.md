# Yuri Rewrite

Yuri Rewrite 是一个 Windows 优先的本地桌面工具，用于导入 TXT 小说、按批次调用 AI 分析章节内容，并将小说改写成双女主 / 百合版本。

应用采用本地优先设计：小说原文、章节拆分、分析结果、改写稿、一致性资产、模型配置、调用日志和导出设置都保存在用户自己的电脑上。

## 功能特性

- 导入 TXT 小说，支持常见 UTF-8、GBK 文本。
- 自动识别章节标题，并按每 30 章生成一个处理批次。
- 如果没有识别到章节，则按每 10 万字生成一个处理批次。
- 每次只分析或改写当前选中的批次，避免一次性把整本小说发送给 AI。
- 支持 OpenAI 兼容接口，可配置 ChatGPT、Moonshot、MiniMax、智谱、百炼、DeepSeek 等兼容端点。
- 支持 Gemini 适配。
- 支持保存多个模型配置，并可测试当前模型是否可用。
- 小说级“基本设定”：
  - 主角姓名，必填。
  - 其他需要女性化的人名，选填。
  - 身材：平胸 / 巨乳。
  - 体型：萝莉 / 御姐 / 少女。
- 支持“高级设定”，用户可自由补充改写要求。
- 改写阶段会把基本设定和高级设定注入提示词。
- 分析阶段只分析小说原始内容，不注入改写设定。
- 分析结果会生成一致性资产，包括人物关系、人物卡、地点、伏笔和术语表。
- 支持手动编辑并保存一致性资产。
- 支持 AI 调用日志页面，查看思考内容、输出文本和原始响应。
- 支持原文 / 改写文对比页面。
- 支持导出 TXT 和 Markdown。
- 支持指定改写完成后的导出目录。
- Release 构建下隐藏 Windows 控制台窗口。

## 技术栈

- 前端：React + TypeScript + Vite
- 桌面端：Tauri v2
- 后端：Rust
- 本地数据库：SQLite / rusqlite
- 图标：lucide-react

## 开发环境

需要先安装：

- Node.js
- Rust
- Windows WebView2 Runtime

安装依赖：

```powershell
npm install
```

启动开发版：

```powershell
npm run tauri:dev
```

仅构建前端：

```powershell
npm run build
```

构建 Windows Release：

```powershell
npm run tauri:build
```

生成便携版 zip：

```powershell
npm run package:portable
```

便携版默认输出到：

```text
portable/YuriRewrite-v0.1.0-windows-x64.zip
```

## 使用说明

1. 打开 Yuri Rewrite。
2. 点击“导入 TXT”选择小说文件。
3. 在“模型配置”中填写 Base URL、模型名和 API Key。
4. 点击“测试模型”确认配置可用。
5. 点击顶部“设定”填写当前小说的基本设定。
6. 在工作台选择当前批次。
7. 点击“分析”分析当前批次。
8. 分析完成后检查或编辑一致性资产。
9. 点击“改写”改写当前批次。
10. 改写完成后进入“对比”页面检查原文和改写稿。
11. 根据需要导出 TXT 或 Markdown。

如果用户在未填写设定时点击“改写”，应用会主动弹出设定对话框。没有导入小说时，“设定”按钮不可用。

## OpenAI 兼容接口示例

常见 Base URL 示例：

- OpenAI: `https://api.openai.com/v1`
- Moonshot: `https://api.moonshot.cn/v1`
- DeepSeek: `https://api.deepseek.com/v1`
- 百炼 DashScope、智谱、MiniMax：填写各平台提供的 OpenAI-compatible Base URL。

模型名需要填写供应商当前提供的具体模型 ID。

## 本地数据与隐私

- 应用数据存放在当前 Windows 用户的应用数据目录中。
- 替换程序目录不会删除已导入小说、配置和分析数据。
- API Key 会优先存储到 Windows 安全存储；不可用时回退到本地数据库。
- AI 日志会保存请求结果和模型输出，但不应包含 API Key 或 Authorization 信息。
- 删除小说时，会同步删除该小说相关的章节、批次文件、设定、一致性资产、任务、日志和改写数据。

## 分发说明

给其他 Windows 用户使用时，发送便携版 zip 即可：

1. 用户解压 zip 到任意目录。
2. 双击 `yuri-rewrite.exe` 启动。
3. 首次使用时在软件内配置模型和 API Key。

当前应用未签名，Windows SmartScreen 可能提示未知发布者。
