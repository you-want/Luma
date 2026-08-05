import { LoaderCircle } from "lucide-react";
import { formatBytes, formatNumber } from "../lib/format";
import type { ScanProgress as ScanProgressType } from "../types/scan";

export function ScanProgress({ progress }: { progress?: ScanProgressType }) {
  return (
    <section className="progress-panel" aria-live="polite">
      <div className="progress-title">
        <LoaderCircle className="spin" size={22} />
        <div>
          <strong>正在整理空间地图</strong>
          <span title={progress?.currentPath}>{progress?.currentPath || "正在读取目录..."}</span>
        </div>
      </div>
      <div className="progress-track" aria-hidden="true">
        <span />
      </div>
      <dl className="progress-stats">
        <div><dt>文件</dt><dd>{formatNumber(progress?.filesScanned ?? 0)}</dd></div>
        <div><dt>目录</dt><dd>{formatNumber(progress?.directoriesScanned ?? 0)}</dd></div>
        <div><dt>已读取</dt><dd>{formatBytes(progress?.bytesScanned ?? 0)}</dd></div>
        <div><dt>跳过错误</dt><dd>{formatNumber(progress?.errors ?? 0)}</dd></div>
      </dl>
    </section>
  );
}
