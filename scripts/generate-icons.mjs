#!/usr/bin/env node
/**
 * Generate Tauri app icons from the McpMux branding SVG.
 * Requires: sharp (installed as workspace devDependency)
 *
 * Generates:
 *   - 32x32.png (tray icon)
 *   - 128x128.png
 *   - 128x128@2x.png (256x256)
 *   - icon.png (512x512)
 *   - icon.ico (multi-resolution Windows icon)
 *   - Square*.png (Windows Store icons)
 *   - StoreLogo.png (50x50)
 */

import sharp from 'sharp';
import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

const ICONS_DIR = join(import.meta.dirname, '..', 'apps', 'desktop', 'src-tauri', 'icons');
const SVG_FULL = join(import.meta.dirname, '..', '..', 'mcpmux.space', 'branding', 'icons', 'appicon-512.svg');
const SVG_SMALL = join(ICONS_DIR, 'appicon-small.svg');

const svgFullBuffer = readFileSync(SVG_FULL);
const svgSmallBuffer = readFileSync(SVG_SMALL);

// Use simplified SVG for sizes ≤ 64px (taskbar/tray), full detail for larger
const SMALL_THRESHOLD = 64;
function svgFor(size) {
  return size <= SMALL_THRESHOLD ? svgSmallBuffer : svgFullBuffer;
}

// PNG sizes to generate
const pngSizes = [
  { name: '32x32.png', size: 32 },
  { name: '128x128.png', size: 128 },
  { name: '128x128@2x.png', size: 256 },
  { name: 'icon.png', size: 512 },
  // Windows Store icons
  { name: 'Square30x30Logo.png', size: 30 },
  { name: 'Square44x44Logo.png', size: 44 },
  { name: 'Square71x71Logo.png', size: 71 },
  { name: 'Square89x89Logo.png', size: 89 },
  { name: 'Square107x107Logo.png', size: 107 },
  { name: 'Square142x142Logo.png', size: 142 },
  { name: 'Square150x150Logo.png', size: 150 },
  { name: 'Square284x284Logo.png', size: 284 },
  { name: 'Square310x310Logo.png', size: 310 },
  { name: 'StoreLogo.png', size: 50 },
];

async function generatePngs() {
  for (const { name, size } of pngSizes) {
    const outPath = join(ICONS_DIR, name);
    const svg = svgFor(size);
    await sharp(svg, { density: Math.round(72 * size / 512 * 4) })
      .resize(size, size)
      .png()
      .toFile(outPath);
    console.log(`  ✓ ${name} (${size}x${size})${size <= SMALL_THRESHOLD ? ' [simplified]' : ''}`);
  }
}

/**
 * Generate a minimal ICO file containing multiple resolutions.
 * ICO format: ICONDIR header + ICONDIRENTRY[] + PNG data chunks
 */
async function generateIco() {
  const icoSizes = [16, 24, 32, 48, 64, 128, 256];
  const pngBuffers = [];

  for (const size of icoSizes) {
    const svg = svgFor(size);
    const buf = await sharp(svg, { density: Math.round(72 * size / 512 * 4) })
      .resize(size, size)
      .png()
      .toBuffer();
    pngBuffers.push({ size, data: buf });
  }

  // ICO file format
  const ICONDIR_SIZE = 6;
  const ICONDIRENTRY_SIZE = 16;
  const headerSize = ICONDIR_SIZE + ICONDIRENTRY_SIZE * pngBuffers.length;

  let totalSize = headerSize;
  for (const { data } of pngBuffers) {
    totalSize += data.length;
  }

  const ico = Buffer.alloc(totalSize);
  let offset = 0;

  // ICONDIR
  ico.writeUInt16LE(0, offset); offset += 2;      // Reserved
  ico.writeUInt16LE(1, offset); offset += 2;      // Type: 1 = ICO
  ico.writeUInt16LE(pngBuffers.length, offset); offset += 2; // Count

  // ICONDIRENTRY for each image
  let dataOffset = headerSize;
  for (const { size, data } of pngBuffers) {
    ico.writeUInt8(size >= 256 ? 0 : size, offset); offset += 1;  // Width
    ico.writeUInt8(size >= 256 ? 0 : size, offset); offset += 1;  // Height
    ico.writeUInt8(0, offset); offset += 1;       // Color palette
    ico.writeUInt8(0, offset); offset += 1;       // Reserved
    ico.writeUInt16LE(1, offset); offset += 2;    // Color planes
    ico.writeUInt16LE(32, offset); offset += 2;   // Bits per pixel
    ico.writeUInt32LE(data.length, offset); offset += 4; // Size of PNG data
    ico.writeUInt32LE(dataOffset, offset); offset += 4;  // Offset to PNG data
    dataOffset += data.length;
  }

  // PNG data
  for (const { data } of pngBuffers) {
    data.copy(ico, offset);
    offset += data.length;
  }

  const outPath = join(ICONS_DIR, 'icon.ico');
  writeFileSync(outPath, ico);
  console.log(`  ✓ icon.ico (${icoSizes.join(', ')}px)`);
}

async function main() {
  console.log('Generating McpMux icons from branding SVG...');
  console.log(`  Full: ${SVG_FULL}`);
  console.log(`  Small (≤${SMALL_THRESHOLD}px): ${SVG_SMALL}`);
  console.log(`  Output: ${ICONS_DIR}\n`);

  await generatePngs();
  await generateIco();

  console.log('\nDone! All icons generated.');
  console.log('Note: icon.icns (macOS) should be generated on macOS using iconutil.');
}

main().catch(console.error);
