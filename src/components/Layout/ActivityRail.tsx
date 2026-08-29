import {
  BookOpenText,
  ChartLine,
  Columns2,
  Cpu,
  FlaskConical,
  Github,
  HelpCircle,
  Loader2,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  ScrollText,
  SlidersHorizontal,
  SquarePen,
  Sun
} from "lucide-react";
import { forwardRef } from "react";
import type { ActiveView } from "./navigation";

type ActivityRailProps = {
  activeView: ActiveView;
  contextCollapsed: boolean;
  theme: "light" | "dark";
  canCompare: boolean;
  canOpenRewriteAb: boolean;
  rewriteAbBusy: boolean;
  rewriteAbProgress?: string;
  rewriteAbRunning?: boolean;
  taskProgress?: number;
  hasAvailableUpdate: boolean;
  updateBusy: boolean;
  systemBusy: boolean;
  onNavigate: (view: ActiveView) => void;
  onOpenRewriteAb: () => void;
  onToggleContext: () => void;
  onToggleTheme: () => void;
  onOpenHelp: () => void;
  onCheckUpdates: () => void;
  onOpenGithub: () => void;
  onOpenTokenStats: () => void;
};

const RailButton = forwardRef<HTMLButtonElement, React.ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean }>(function RailButton(props, ref) {
  const { label, active, className = "", children, ...buttonProps } = props;
  return (
    <button
      ref={ref}
      {...buttonProps}
      className={`activity-rail-button${active ? " active" : ""}${className ? ` ${className}` : ""}`}
      aria-label={label}
      title={label}
    >
      {children}
      <span className="activity-tooltip" role="tooltip">{label}</span>
    </button>
  );
});

export function ActivityRail(props: ActivityRailProps) {
  const navigate = (view: ActiveView) => {
    props.onNavigate(view);
  };

  return (
    <nav className="activity-rail" aria-label="全局功能">
      <RailButton label="Yuri Rewrite 主页" className="activity-brand" onClick={() => navigate("workspace")}>
        <BookOpenText size={18} />
      </RailButton>
      <div className="activity-rail-primary">
        <RailButton label="工作台" active={props.activeView === "workspace"} onClick={() => navigate("workspace")}>
          <SquarePen size={18} />
          {props.taskProgress !== undefined && <span className="activity-progress-dot" aria-hidden="true" title={`任务进度 ${props.taskProgress}%`} />}
        </RailButton>
        <RailButton label="对比" active={props.activeView === "compare"} disabled={!props.canCompare} onClick={() => navigate("compare")}>
          <Columns2 size={18} />
        </RailButton>
        <RailButton label={props.rewriteAbProgress ? `A/B 实验，${props.rewriteAbRunning ? "运行中，" : ""}候选 ${props.rewriteAbProgress}` : "A/B 实验"} active={props.activeView === "rewrite-ab"} disabled={!props.canOpenRewriteAb} onClick={props.onOpenRewriteAb}>
          {props.rewriteAbBusy ? <Loader2 className="spin" size={18} /> : <FlaskConical size={18} />}
          {props.rewriteAbProgress && <span className="activity-mini-count">{props.rewriteAbProgress}</span>}
        </RailButton>
        <RailButton label="管理模型" active={props.activeView === "models"} onClick={() => navigate("models")}>
          <Cpu size={18} />
        </RailButton>
        <RailButton label="设置" active={props.activeView === "settings"} onClick={() => navigate("settings")}>
          <SlidersHorizontal size={18} />
        </RailButton>
      </div>
      <div className="activity-rail-secondary">
        <RailButton label={props.contextCollapsed ? "展开侧栏" : "折叠侧栏"} onClick={props.onToggleContext}>
          {props.contextCollapsed ? <PanelLeftOpen size={17} /> : <PanelLeftClose size={17} />}
        </RailButton>
        <RailButton label={props.theme === "dark" ? "日间模式" : "夜间模式"} aria-pressed={props.theme === "dark"} onClick={props.onToggleTheme}>
          {props.theme === "dark" ? <Sun size={17} /> : <Moon size={17} />}
        </RailButton>
        <RailButton label="日志" active={props.activeView === "logs"} onClick={() => navigate("logs")}>
          <ScrollText size={17} />
        </RailButton>
        <RailButton label="Token统计" active={props.activeView === "token-stats"} onClick={props.onOpenTokenStats} disabled={props.systemBusy}>
          <ChartLine size={17} />
        </RailButton>
        <RailButton label="帮助" onClick={props.onOpenHelp}>
          <HelpCircle size={17} />
        </RailButton>
        <RailButton label="检查更新" onClick={props.onCheckUpdates} disabled={props.systemBusy}>
          {props.updateBusy ? <Loader2 className="spin" size={17} /> : <RefreshCw size={17} />}
          {props.hasAvailableUpdate && <span className="activity-update-dot" aria-label="发现新版本" />}
        </RailButton>
        <RailButton label="GitHub" onClick={props.onOpenGithub} disabled={props.systemBusy}>
          <Github size={17} />
        </RailButton>
      </div>
    </nav>
  );
}
