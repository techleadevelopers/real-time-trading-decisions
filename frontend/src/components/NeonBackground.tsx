import { useEffect, useRef } from 'react';

export default function NeonBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    let animId: number;
    let t = 0;

    const resize = () => {
      canvas.width = window.innerWidth;
      canvas.height = window.innerHeight;
    };
    resize();
    window.addEventListener('resize', resize);

    // Particle system
    const N = 60;
    const particles: { x: number; y: number; vx: number; vy: number; r: number; a: number; color: string }[] = [];
    const colors = ['rgba(0,200,255,', 'rgba(0,255,136,', 'rgba(155,89,255,'];

    for (let i = 0; i < N; i++) {
      particles.push({
        x: Math.random() * window.innerWidth,
        y: Math.random() * window.innerHeight,
        vx: (Math.random() - 0.5) * 0.3,
        vy: (Math.random() - 0.5) * 0.3,
        r: Math.random() * 1.4 + 0.3,
        a: Math.random(),
        color: colors[Math.floor(Math.random() * colors.length)],
      });
    }

    const GRID_SIZE = 80;

    const draw = () => {
      t += 0.004;
      const w = canvas.width;
      const h = canvas.height;

      ctx.clearRect(0, 0, w, h);

      // Background
      ctx.fillStyle = '#020408';
      ctx.fillRect(0, 0, w, h);

      // Animated grid
      ctx.save();
      const cols = Math.ceil(w / GRID_SIZE) + 1;
      const rows = Math.ceil(h / GRID_SIZE) + 1;
      const offsetX = (t * 8) % GRID_SIZE;
      const offsetY = (t * 4) % GRID_SIZE;

      for (let c = 0; c < cols; c++) {
        const x = c * GRID_SIZE - offsetX;
        const alpha = 0.018 + 0.008 * Math.sin(t + c * 0.3);
        ctx.strokeStyle = `rgba(0,200,255,${alpha})`;
        ctx.lineWidth = 0.5;
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, h);
        ctx.stroke();
      }

      for (let r = 0; r < rows; r++) {
        const y = r * GRID_SIZE - offsetY;
        const alpha = 0.018 + 0.008 * Math.sin(t + r * 0.2);
        ctx.strokeStyle = `rgba(0,200,255,${alpha})`;
        ctx.lineWidth = 0.5;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(w, y);
        ctx.stroke();
      }
      ctx.restore();

      // Scanline sweep
      const scanY = ((t * 60) % (h + 100)) - 50;
      const scanGrad = ctx.createLinearGradient(0, scanY - 30, 0, scanY + 30);
      scanGrad.addColorStop(0, 'rgba(0,200,255,0)');
      scanGrad.addColorStop(0.5, 'rgba(0,200,255,0.025)');
      scanGrad.addColorStop(1, 'rgba(0,200,255,0)');
      ctx.fillStyle = scanGrad;
      ctx.fillRect(0, scanY - 30, w, 60);

      // Particles
      for (const p of particles) {
        p.x += p.vx;
        p.y += p.vy;
        if (p.x < 0) p.x = w;
        if (p.x > w) p.x = 0;
        if (p.y < 0) p.y = h;
        if (p.y > h) p.y = 0;
        p.a = 0.3 + 0.3 * Math.sin(t * 1.5 + p.x * 0.01);

        ctx.beginPath();
        ctx.arc(p.x, p.y, p.r, 0, Math.PI * 2);
        ctx.fillStyle = `${p.color}${p.a.toFixed(2)})`;
        ctx.fill();
      }

      // Connect close particles
      for (let i = 0; i < N; i++) {
        for (let j = i + 1; j < N; j++) {
          const dx = particles[i].x - particles[j].x;
          const dy = particles[i].y - particles[j].y;
          const dist = Math.sqrt(dx * dx + dy * dy);
          if (dist < 120) {
            const a = (1 - dist / 120) * 0.06;
            ctx.strokeStyle = `rgba(0,200,255,${a.toFixed(3)})`;
            ctx.lineWidth = 0.4;
            ctx.beginPath();
            ctx.moveTo(particles[i].x, particles[i].y);
            ctx.lineTo(particles[j].x, particles[j].y);
            ctx.stroke();
          }
        }
      }

      // Corner glow accents
      const glow = (cx: number, cy: number, color: string) => {
        const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, 300);
        g.addColorStop(0, `${color}0.06)`);
        g.addColorStop(1, `${color}0)`);
        ctx.fillStyle = g;
        ctx.fillRect(cx - 300, cy - 300, 600, 600);
      };
      glow(0, 0, 'rgba(0,200,255,');
      glow(w, h, 'rgba(155,89,255,');

      animId = requestAnimationFrame(draw);
    };

    draw();

    return () => {
      cancelAnimationFrame(animId);
      window.removeEventListener('resize', resize);
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      style={{
        position: 'fixed',
        inset: 0,
        zIndex: 0,
        pointerEvents: 'none',
        opacity: 0.9,
      }}
    />
  );
}
