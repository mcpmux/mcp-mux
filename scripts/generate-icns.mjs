#!/usr/bin/env node
/**
 * Generate a macOS ICNS file from the McpMux branding SVG.
 * ICNS format with PNG payloads for modern macOS.
 */

import sharp from 'sharp';
import { readFileSync, writeFileSync } from 'fs';
import { join } from 'path';

const ICONS_DIR = join(import.meta.dirname, '..', 'apps', 'desktop', 'src-tauri', 'icons');
const SVG_FULL = join(import.meta.dirname, '..', '..', 'mcpmux.space', 'branding', 'icons', 'appicon-512.svg');
const SVG_SMALL = join(ICONS_DIR, 'appicon-small.svg');

const svgFullBuffer = readFileSync(SVG_FULL);
const svgSmallBuffer = readFileSync(SVG_SMALL);

const SMALL_THRESHOLD = 64;
function svgFor(size) {
  return size <= SMALL_THRESHOLD ? svgSmallBuffer : svgFullBuffer;
}

// ICNS icon types that accept PNG data
const ICNS_TYPES = [
  { type: 'ic07', size: 128 },   // 128x128
  { type: 'ic08', size: 256 },   // 256x256
  { type: 'ic09', size: 512 },   // 512x512
  { type: 'ic10', size: 1024 },  // 1024x1024 (512@2x)
  { type: 'ic11', size: 32 },    // 16x16@2x
  { type: 'ic12', size: 64 },    // 32x32@2x
  { type: 'ic13', size: 256 },   // 128x128@2x
  { type: 'ic14', size: 512 },   // 256x256@2x
];

async function main() {
  console.log('Generating icon.icns...');

  const entries = [];
  for (const { type, size } of ICNS_TYPES) {
    const svg = svgFor(size);
    const png = await sharp(svg, { density: Math.round(72 * size / 512 * 4) })
      .resize(size, size)
      .png()
      .toBuffer();
    entries.push({ type, data: png });
  }

  // Calculate total size
  let totalSize = 8; // ICNS header
  for (const { data } of entries) {
    totalSize += 8 + data.length; // type(4) + length(4) + data
  }

  const icns = Buffer.alloc(totalSize);
  let offset = 0;

  // ICNS magic header
  icns.write('icns', offset, 'ascii'); offset += 4;
  icns.writeUInt32BE(totalSize, offset); offset += 4;

  // Write each entry
  for (const { type, data } of entries) {
    icns.write(type, offset, 'ascii'); offset += 4;
    icns.writeUInt32BE(8 + data.length, offset); offset += 4;
    data.copy(icns, offset); offset += data.length;
  }

  const outPath = join(ICONS_DIR, 'icon.icns');
  writeFileSync(outPath, icns);
  console.log(`  ✓ icon.icns (${totalSize} bytes, ${entries.length} sizes)`);
}

main().catch(console.error);
