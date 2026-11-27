import { useLayoutEffect, useRef, useState } from "react";

/** Measured SVG connectors between an ordered row of flow nodes. */
export function useFlowGeometry(ids: string[]) {
  const boardRef = useRef<HTMLDivElement>(null);
  const nodeRefs = useRef(new Map<string, HTMLDivElement>());
  const [paths, setPaths] = useState<Array<{ id: string; d: string }>>([]);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const key = JSON.stringify(ids);

  useLayoutEffect(() => {
    const board = boardRef.current;
    if (!board) return;
    let disposed = false;
    let frame: number | null = null;
    const measure = () => {
      if (disposed) return;
      const bounds = board.getBoundingClientRect();
      const width = board.clientWidth || bounds.width;
      const height = board.clientHeight || bounds.height;
      const scaleX = bounds.width > 0 ? width / bounds.width : 1;
      const scaleY = bounds.height > 0 ? height / bounds.height : 1;
      const next: Array<{ id: string; d: string }> = [];
      for (let index = 0; index < ids.length - 1; index += 1) {
        const from = nodeRefs.current.get(ids[index]);
        const to = nodeRefs.current.get(ids[index + 1]);
        if (!from || !to) continue;
        const a = from.getBoundingClientRect();
        const b = to.getBoundingClientRect();
        const x1 = (a.right - bounds.left) * scaleX;
        const y1 = (a.top + a.height / 2 - bounds.top) * scaleY;
        const x2 = (b.left - bounds.left) * scaleX;
        const y2 = (b.top + b.height / 2 - bounds.top) * scaleY;
        if (x2 <= x1) continue;
        const bend = Math.min(26, (x2 - x1) * 0.45);
        next.push({
          id: `${ids[index]}-${ids[index + 1]}`,
          d: `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`,
        });
      }
      setPaths(next);
      setSize({ width, height });
    };
    const schedule = () => {
      if (typeof requestAnimationFrame === "undefined") {
        measure();
        return;
      }
      if (frame !== null) cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => {
        frame = null;
        measure();
      });
    };
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(schedule);
    observer?.observe(board);
    ids.forEach((id) => {
      const node = nodeRefs.current.get(id);
      if (node) observer?.observe(node);
    });
    measure();
    window.addEventListener("resize", schedule);
    return () => {
      disposed = true;
      observer?.disconnect();
      if (frame !== null) cancelAnimationFrame(frame);
      window.removeEventListener("resize", schedule);
    };
  }, [key]);

  return { boardRef, nodeRefs, paths, size };
}
