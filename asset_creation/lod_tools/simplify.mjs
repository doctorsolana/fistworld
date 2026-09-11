// Offline spatial simplification merges nearby details without erasing thin walls.
import { readFileSync } from 'node:fs';
import { MeshoptSimplifier } from 'meshoptimizer';

await MeshoptSimplifier.ready;
const input = JSON.parse(readFileSync(0, 'utf8'));
const indices = new Uint32Array(input.indices);
const positions = new Float32Array(input.positions.flat());
const target = Math.max(3, Math.floor(indices.length * input.ratio / 3) * 3);
const [result, error] = MeshoptSimplifier.simplifySloppy(
  indices, positions, 3, null, target, input.error,
);
process.stdout.write(JSON.stringify({ indices: Array.from(result), error }));
