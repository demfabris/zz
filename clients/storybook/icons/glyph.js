(function (root) {
  const SAMPLES = 16;
  const CORNER = 0.45;

  function num(value) {
    const text = value.toFixed(2).replace(/\.?0+$/, "");
    return text === "-0" || text === "" ? "0" : text;
  }

  function outline(points, closed, radius, smoothing, cap) {
    const count = points.length;
    let d = "";
    const emit = (x, y) => {
      d += (d ? "L" : "M") + num(x) + " " + num(y);
    };
    for (let index = 0; index < count; index++) {
      const [x, y, scale = 0] = points[index];
      const inner = closed || (index > 0 && index < count - 1);
      if (!inner || scale <= 0 || radius <= 0) {
        emit(x, y);
        continue;
      }
      const [px, py] = points[(index + count - 1) % count];
      const [nx, ny] = points[(index + 1) % count];
      const incoming = Math.hypot(px - x, py - y);
      const outgoing = Math.hypot(nx - x, ny - y);
      const r = Math.min(radius * scale, cap * Math.min(incoming, outgoing));
      const bx = (px - x) / incoming;
      const by = (py - y) / incoming;
      const ax = (nx - x) / outgoing;
      const ay = (ny - y) / outgoing;
      for (let step = 0; step <= SAMPLES; step++) {
        const theta = (Math.PI / 2) * (1 - step / SAMPLES);
        const along = 1 - Math.pow(Math.max(0, Math.cos(theta)), 2 / smoothing);
        const across = 1 - Math.pow(Math.max(0, Math.sin(theta)), 2 / smoothing);
        emit(x + bx * r * along + ax * r * across, y + by * r * along + ay * r * across);
      }
    }
    return closed ? d + "Z" : d;
  }

  function gear(teeth, rootRadius, tip, tipWidth, rootWidth, scale = 0.5) {
    const period = (Math.PI * 2) / teeth;
    const points = [];
    for (let tooth = 0; tooth < teeth; tooth++) {
      const center = tooth * period;
      for (const [angle, reach] of [
        [center - (rootWidth * period) / 2, rootRadius],
        [center - (tipWidth * period) / 2, tip],
        [center + (tipWidth * period) / 2, tip],
        [center + (rootWidth * period) / 2, rootRadius],
      ]) {
        points.push([12 + reach * Math.sin(angle), 12 - reach * Math.cos(angle), scale]);
      }
    }
    return points;
  }

  function part(kind, args, radius, smoothing) {
    const path = (d, extra = "") => `<path d="${d}"${extra}/>`;
    switch (kind) {
      case "line":
        return path(`M${num(args[0])} ${num(args[1])}L${num(args[2])} ${num(args[3])}`);
      case "path":
        return path(outline(args, false, radius, smoothing, CORNER));
      case "shape":
        return path(outline(args, true, radius, smoothing, CORNER));
      case "rect": {
        const [x0, y0, x1, y1, scale = 1] = args;
        const corners = [[x0, y0, scale], [x1, y0, scale], [x1, y1, scale], [x0, y1, scale]];
        return path(outline(corners, true, radius, smoothing, CORNER));
      }
      case "circle":
        return `<circle cx="${num(args[0])}" cy="${num(args[1])}" r="${num(args[2])}"/>`;
      case "ellipse":
        return `<ellipse cx="${num(args[0])}" cy="${num(args[1])}" rx="${num(args[2])}" ry="${num(args[3])}"/>`;
      case "arc": {
        const [cx, cy, r, from, to] = args;
        const at = (deg) => [cx + r * Math.cos((deg * Math.PI) / 180), cy + r * Math.sin((deg * Math.PI) / 180)];
        const [sx, sy] = at(from);
        const [ex, ey] = at(to);
        const large = to - from > 180 ? 1 : 0;
        return path(`M${num(sx)} ${num(sy)}A${num(r)} ${num(r)} 0 ${large} 1 ${num(ex)} ${num(ey)}`);
      }
      case "dot": {
        const [cx, cy, size = 2] = args;
        const half = size / 2;
        const corners = [[cx - half, cy - half, 1], [cx + half, cy - half, 1], [cx + half, cy + half, 1], [cx - half, cy + half, 1]];
        return path(outline(corners, true, Math.min(radius, half), smoothing, 0.5), ' fill="currentColor" stroke="none"');
      }
      case "gear":
        return path(outline(gear(...args), true, radius, smoothing, CORNER));
      case "curve":
        return path(`M${num(args[0])} ${num(args[1])}Q${num(args[2])} ${num(args[3])} ${num(args[4])} ${num(args[5])}`);
      case "cubic":
        return path(`M${num(args[0])} ${num(args[1])}C${args.slice(2).map(num).join(" ")}`);
      case "solid":
        return part(args[0][0], args[0].slice(1), radius, smoothing).replace(/\/>$/, ' fill="currentColor" stroke="none"/>');
      case "fade":
        return part(args[1][0], args[1].slice(1), radius, smoothing).replace(/\/>$/, ` opacity="${num(args[0])}"/>`);
      case "d":
        return path(args[0]);
      default:
        throw new Error(`unknown part ${kind}`);
    }
  }

  function svg(set, icon, radius, smoothing) {
    const round = radius > 0;
    const body = icon.parts.map(([kind, ...args]) => part(kind, args, radius, Math.max(smoothing, 2))).join("");
    return (
      `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="${set.stroke}" ` +
      `stroke-linecap="${round ? "round" : "square"}" stroke-linejoin="${round ? "round" : "miter"}">${body}</svg>`
    );
  }

  root.zzGlyph = { svg };
})(typeof window === "undefined" ? globalThis : window);
