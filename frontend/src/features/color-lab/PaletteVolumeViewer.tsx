/**
 * Color Lab Oklab scatter + static sRGB gamut shell.
 * Conversion is Rust-only (`colorsToOklab`); this module must not implement sRGB→Oklab.
 *
 * Scene mapping (locked Track L): X = a, Y = L (up), Z = b.
 */
import { useEffect, useRef, useState } from 'react';
import * as THREE from 'three';
import { OrbitControls } from 'three/examples/jsm/controls/OrbitControls.js';
import { colorsToOklab, logIpcError } from '../../shared/ipc';
import type { OklabPointDto } from '../../shared/ipc';
import type { ColorEntry } from './types';
import gamutMesh from './assets/srgb-gamut-oklab.json';
import styles from './PaletteVolumeViewer.module.css';
import { bind } from '../../shared/ui/cn';

const cn = bind(styles);

const POINT_RADIUS = 0.018;
const SELECTED_RADIUS = 0.032;

interface CloudPoint {
  colorIndex: number;
  l: number;
  a: number;
  b: number;
  srgb_hex: string;
}

export interface PaletteVolumeViewerProps {
  colors: ColorEntry[];
  selectedIndex: number | null;
  onSelectIndex: (index: number) => void;
  compact?: boolean;
}

function hexToThreeColor(hex: string): THREE.Color {
  const normalized = hex.startsWith('#') ? hex : `#${hex}`;
  return new THREE.Color(normalized);
}

function buildGamutMesh(): THREE.Mesh {
  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute(
    'position',
    new THREE.Float32BufferAttribute(gamutMesh.positions, 3)
  );
  geometry.setIndex(gamutMesh.indices);
  geometry.computeVertexNormals();
  const material = new THREE.MeshBasicMaterial({
    color: 0x9aa3ad,
    transparent: true,
    opacity: 0.12,
    side: THREE.DoubleSide,
    depthWrite: false,
  });
  const mesh = new THREE.Mesh(geometry, material);
  mesh.add(
    new THREE.LineSegments(
      new THREE.WireframeGeometry(geometry),
      new THREE.LineBasicMaterial({
        color: 0xc5ccd4,
        transparent: true,
        opacity: 0.28,
      })
    )
  );
  return mesh;
}

function disposeObject3D(obj: THREE.Object3D) {
  if (obj instanceof THREE.Mesh || obj instanceof THREE.LineSegments) {
    obj.geometry.dispose();
    const mat = obj.material;
    if (Array.isArray(mat)) mat.forEach((m) => m.dispose());
    else mat.dispose();
  }
}

function clearGroup(group: THREE.Group) {
  while (group.children.length > 0) {
    const child = group.children[0];
    group.remove(child);
    disposeObject3D(child);
  }
}

function addCloudMeshes(group: THREE.Group, cloud: CloudPoint[], selectedIndex: number | null) {
  for (const p of cloud) {
    const mesh = new THREE.Mesh(
      new THREE.SphereGeometry(1, 12, 10),
      new THREE.MeshBasicMaterial({ color: hexToThreeColor(p.srgb_hex) })
    );
    mesh.position.set(p.a, p.l, p.b);
    mesh.scale.setScalar(selectedIndex === p.colorIndex ? SELECTED_RADIUS : POINT_RADIUS);
    mesh.userData.colorIndex = p.colorIndex;
    group.add(mesh);
  }
}

export default function PaletteVolumeViewer({
  colors,
  selectedIndex,
  onSelectIndex,
  compact = false,
}: PaletteVolumeViewerProps) {
  const wrapRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const selectedRef = useRef(selectedIndex);
  const onSelectRef = useRef(onSelectIndex);
  const cloudRef = useRef<CloudPoint[]>([]);
  const pointsGroupRef = useRef<THREE.Group | null>(null);
  const [sectionOpen, setSectionOpen] = useState(!compact);
  selectedRef.current = selectedIndex;
  onSelectRef.current = onSelectIndex;

  const validCount = colors.reduce((n, c) => n + (c.valid ? 1 : 0), 0);

  useEffect(() => {
    setSectionOpen(!compact);
  }, [compact]);

  useEffect(() => {
    const valid = colors
      .map((c, colorIndex) => ({ c, colorIndex }))
      .filter(({ c }) => c.valid);
    if (valid.length === 0) {
      cloudRef.current = [];
      return;
    }
    let cancelled = false;
    void colorsToOklab(valid.map(({ c }) => c.hex))
      .then((points: OklabPointDto[]) => {
        if (cancelled) return;
        cloudRef.current = points.map((p, i) => ({
          colorIndex: valid[i].colorIndex,
          l: p.l,
          a: p.a,
          b: p.b,
          srgb_hex: p.srgb_hex,
        }));
        const group = pointsGroupRef.current;
        if (group) {
          clearGroup(group);
          addCloudMeshes(group, cloudRef.current, selectedRef.current);
        }
      })
      .catch((err) => {
        if (!cancelled) logIpcError('PaletteVolumeViewer.colorsToOklab', err);
      });
    return () => {
      cancelled = true;
    };
  }, [colors]);

  useEffect(() => {
    const group = pointsGroupRef.current;
    if (!group) return;
    for (const child of group.children) {
      if (!(child instanceof THREE.Mesh)) continue;
      const selected = selectedRef.current === child.userData.colorIndex;
      child.scale.setScalar(selected ? SELECTED_RADIUS : POINT_RADIUS);
    }
  }, [selectedIndex]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const wrap = wrapRef.current;
    if (!canvas || !wrap || validCount === 0) return;

    let renderer: THREE.WebGLRenderer;
    try {
      renderer = new THREE.WebGLRenderer({
        canvas,
        antialias: true,
        alpha: true,
      });
    } catch {
      return;
    }

    renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
    const scene = new THREE.Scene();
    const camera = new THREE.PerspectiveCamera(45, 1, 0.01, 20);
    camera.position.set(0.55, 0.95, 0.55);
    camera.up.set(0, 1, 0);

    const controls = new OrbitControls(camera, canvas);
    controls.enableDamping = true;
    controls.target.set(0, 0.5, 0);
    controls.minDistance = 0.25;
    controls.maxDistance = 4;

    const axes = new THREE.AxesHelper(0.35);
    scene.add(axes);
    scene.add(buildGamutMesh());

    const pointsGroup = new THREE.Group();
    scene.add(pointsGroup);
    pointsGroupRef.current = pointsGroup;
    addCloudMeshes(pointsGroup, cloudRef.current, selectedRef.current);

    const raycaster = new THREE.Raycaster();
    const pointer = new THREE.Vector2();
    let pointerDown = new THREE.Vector2();
    let dragging = false;

    const setSize = () => {
      const w = wrap.clientWidth || 1;
      const h = wrap.clientHeight || 1;
      renderer.setSize(w, h, false);
      camera.aspect = w / h;
      camera.updateProjectionMatrix();
    };
    setSize();
    const ro = new ResizeObserver(setSize);
    ro.observe(wrap);

    const onPointerDown = (ev: PointerEvent) => {
      dragging = false;
      pointerDown.set(ev.offsetX, ev.offsetY);
    };
    const onPointerMove = (ev: PointerEvent) => {
      if (ev.buttons === 0) return;
      const dx = ev.offsetX - pointerDown.x;
      const dy = ev.offsetY - pointerDown.y;
      if (dx * dx + dy * dy > 16) dragging = true;
    };
    const onPointerUp = (ev: PointerEvent) => {
      if (dragging) return;
      const rect = canvas.getBoundingClientRect();
      pointer.x = ((ev.clientX - rect.left) / rect.width) * 2 - 1;
      pointer.y = -((ev.clientY - rect.top) / rect.height) * 2 + 1;
      raycaster.setFromCamera(pointer, camera);
      const hits = raycaster.intersectObjects(pointsGroup.children, false);
      if (hits.length > 0) {
        const idx = hits[0].object.userData.colorIndex;
        if (typeof idx === 'number') onSelectRef.current(idx);
      }
    };
    canvas.addEventListener('pointerdown', onPointerDown);
    canvas.addEventListener('pointermove', onPointerMove);
    canvas.addEventListener('pointerup', onPointerUp);

    let raf = 0;
    const tick = () => {
      controls.update();
      renderer.render(scene, camera);
      raf = requestAnimationFrame(tick);
    };
    tick();

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      canvas.removeEventListener('pointerdown', onPointerDown);
      canvas.removeEventListener('pointermove', onPointerMove);
      canvas.removeEventListener('pointerup', onPointerUp);
      pointsGroupRef.current = null;
      controls.dispose();
      renderer.dispose();
      scene.traverse((obj) => {
        disposeObject3D(obj);
      });
    };
  }, [validCount, compact]);

  if (validCount === 0) return null;

  return (
    <details
      className={cn('volume-section')}
      open={sectionOpen}
      onToggle={(e) => setSectionOpen((e.target as HTMLDetailsElement).open)}
    >
      <summary className={cn('volume-summary')}>oklab volume</summary>
      <div
        ref={wrapRef}
        className={cn('volume-canvas-wrap', compact && 'volume-canvas-wrap-compact')}
      >
        <canvas
          ref={canvasRef}
          className={cn('volume-canvas')}
          aria-label="Oklab palette volume"
        />
      </div>
      <div className={cn('volume-hint')}>Y = L · click a point to select</div>
    </details>
  );
}
