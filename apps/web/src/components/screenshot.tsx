import { toCanvas } from "html-to-image";
import { useEffect, useRef, useState, type PointerEvent } from "react";
import type { Screenshot } from "@/api/contract";
import { Button } from "./ui/button";

/** The service takes at most 3 MB of image. */
const MAX_BYTES = 3 * 1024 * 1024;

/** The color behind the app, light or dark. */
function pageBackground(): string {
  for (const element of [document.body, document.documentElement]) {
    const color = getComputedStyle(element).backgroundColor;
    if (color && color !== "transparent" && !/rgba\(.*,\s*0\)$/.test(color)) return color;
  }
  return "#ffffff";
}

/**
 * A picture of what the person sees now: the app, without the feedback
 * panel, which lives outside #root.
 */
export async function capturePage(): Promise<HTMLCanvasElement> {
  const root = document.getElementById("root");
  if (!root) throw new Error("The page could not be captured.");
  const background = pageBackground();
  const page = await toCanvas(root, { pixelRatio: 1, backgroundColor: background });
  // Only the part on screen, on an opaque background, so a blur hides everything.
  const view = document.createElement("canvas");
  view.width = Math.min(window.innerWidth, page.width);
  view.height = Math.min(window.innerHeight, page.height);
  const ctx = view.getContext("2d");
  if (ctx) {
    ctx.fillStyle = background;
    ctx.fillRect(0, 0, view.width, view.height);
    ctx.drawImage(page, window.scrollX, window.scrollY, view.width, view.height, 0, 0, view.width, view.height);
  }
  return view;
}

/**
 * Blurs a part of the image so strongly that no text in it can be read. The
 * part is shrunk to a few pixels and drawn back smoothly, fully opaque, so
 * nothing of the original shows through, not even at the edges.
 */
function blur(canvas: HTMLCanvasElement, x: number, y: number, w: number, h: number): void {
  const ctx = canvas.getContext("2d");
  if (!ctx || w < 1 || h < 1) return;
  const small = document.createElement("canvas");
  small.width = Math.max(1, Math.round(w / 24));
  small.height = Math.max(1, Math.round(h / 24));
  const sctx = small.getContext("2d");
  if (!sctx) return;
  // Opaque underneath, so no original pixel can show through.
  sctx.fillStyle = "#ffffff";
  sctx.fillRect(0, 0, small.width, small.height);
  sctx.imageSmoothingEnabled = true;
  sctx.drawImage(canvas, x, y, w, h, 0, 0, small.width, small.height);
  ctx.save();
  ctx.beginPath();
  ctx.rect(x, y, w, h);
  ctx.clip();
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.drawImage(small, 0, 0, small.width, small.height, x, y, w, h);
  ctx.restore();
}

function base64Of(canvas: HTMLCanvasElement, type: string, quality?: number): string {
  return canvas.toDataURL(type, quality).split(",")[1] ?? "";
}

const bytesOf = (data: string) => Math.floor((data.length * 3) / 4);

/** The image as the service takes it: PNG, or JPEG when PNG is too big. */
export function encodeScreenshot(canvas: HTMLCanvasElement): Screenshot {
  const png = base64Of(canvas, "image/png");
  if (bytesOf(png) <= MAX_BYTES) return { type: "image/png", data: png };
  for (const quality of [0.85, 0.7, 0.5]) {
    const jpeg = base64Of(canvas, "image/jpeg", quality);
    if (bytesOf(jpeg) <= MAX_BYTES) return { type: "image/jpeg", data: jpeg };
  }
  const half = document.createElement("canvas");
  half.width = Math.round(canvas.width / 2);
  half.height = Math.round(canvas.height / 2);
  half.getContext("2d")?.drawImage(canvas, 0, 0, half.width, half.height);
  return { type: "image/jpeg", data: base64Of(half, "image/jpeg", 0.7) };
}

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Shows the screenshot and lets the person drag over any part to blur it.
 * The blur goes into the image itself, so what is sent is what they see.
 */
export function ScreenshotEditor({ source, onChange }: { source: HTMLCanvasElement; onChange: (canvas: HTMLCanvasElement) => void }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const history = useRef<ImageData[]>([]);
  const [start, setStart] = useState<{ x: number; y: number } | null>(null);
  const [box, setBox] = useState<Box | null>(null);
  // Screen pixels per image pixel, to draw the box being dragged.
  const [shown, setShown] = useState(1);
  const [steps, setSteps] = useState(0);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    canvas.width = source.width;
    canvas.height = source.height;
    canvas.getContext("2d")?.drawImage(source, 0, 0);
    history.current = [];
    setSteps(0);
    onChange(canvas);
  }, [source, onChange]);

  // Pointer position in image pixels.
  const at = (e: PointerEvent<HTMLCanvasElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const scale = e.currentTarget.width / rect.width;
    return { x: (e.clientX - rect.left) * scale, y: (e.clientY - rect.top) * scale };
  };
  const boxFrom = (a: { x: number; y: number }, b: { x: number; y: number }): Box => ({ x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), w: Math.abs(a.x - b.x), h: Math.abs(a.y - b.y) });

  const finish = () => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (canvas && ctx && box && box.w > 4 && box.h > 4) {
      history.current.push(ctx.getImageData(0, 0, canvas.width, canvas.height));
      blur(canvas, box.x, box.y, box.w, box.h);
      setSteps(history.current.length);
      onChange(canvas);
    }
    setStart(null);
    setBox(null);
  };

  const undo = () => {
    const canvas = canvasRef.current;
    const last = history.current.pop();
    if (canvas && last) {
      canvas.getContext("2d")?.putImageData(last, 0, 0);
      setSteps(history.current.length);
      onChange(canvas);
    }
  };

  return (
    <div>
      <div className="relative overflow-hidden rounded-md border border-line">
        <canvas
          ref={canvasRef}
          data-testid="screenshot"
          aria-label="Screenshot. Drag over any part to blur it."
          className="block h-auto w-full cursor-crosshair touch-none select-none"
          onPointerDown={(e) => {
            e.currentTarget.setPointerCapture(e.pointerId);
            setShown(e.currentTarget.getBoundingClientRect().width / e.currentTarget.width);
            const p = at(e);
            setStart(p);
            setBox({ x: p.x, y: p.y, w: 0, h: 0 });
          }}
          onPointerMove={(e) => {
            if (start) setBox(boxFrom(start, at(e)));
          }}
          onPointerUp={finish}
          onPointerCancel={() => {
            setStart(null);
            setBox(null);
          }}
        />
        {box ? (
          <span
            aria-hidden="true"
            className="pointer-events-none absolute border-2 border-accent bg-accent/15"
            style={{ left: box.x * shown, top: box.y * shown, width: box.w * shown, height: box.h * shown }}
          />
        ) : null}
      </div>
      <div className="mt-2 flex items-center justify-between gap-2 text-[13px] text-muted">
        <span>Drag over any part to blur it. The blur is in the image that is sent.</span>
        <Button size="sm" variant="ghost" disabled={!steps} onClick={undo}>
          Undo blur
        </Button>
      </div>
    </div>
  );
}
