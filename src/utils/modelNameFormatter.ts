import { modelFamilies } from "../data/recommended-models";

/**
 * Utility functions for formatting model names for display
 */

/**
 * Convert a model filename to a user-friendly display name
 * 
 * Examples:
 * - "phi-3-mini-q4.gguf" → "Phi-3 Mini"
 * - "llama-3.2-3b-instruct-q4_k_m.gguf" → "Llama 3.2 3B Instruct"
 * - "mistral-7b-instruct-v0.2-Q4_0.gguf" → "Mistral 7B Instruct v0.2"
 */
export function formatModelDisplayName(filename: string): string {
  // Remove file extension
  let name = filename.replace(/\.gguf$/i, '');
  
  // Remove common quantization patterns at the end
  name = name.replace(/-?(q4|q5|q6|q8|Q4_0|Q4_K_M|Q5_0|Q5_K_M|Q6_K|Q8_0|f16|f32)$/i, '');
  name = name.replace(/-?(q4_k_m|q5_k_m|q4_k_s|q5_k_s|q6_k|q8_0)$/i, '');
  
  // Remove trailing dashes/underscores
  name = name.replace(/[-_]+$/, '');
  
  // Replace dashes and underscores with spaces
  name = name.replace(/[-_]+/g, ' ');
  
  // Capitalize each word, but preserve version numbers and special patterns
  name = name.split(' ').map(word => {
    // Don't capitalize if it's a version number pattern (e.g., "v0.2", "3.2")
    if (/^v?\d+(\.\d+)*$/.test(word)) {
      return word;
    }
    
    // Keep size indicators uppercase (e.g., "3B", "7B", "13B")
    if (/^\d+[bB]$/.test(word)) {
      return word.toUpperCase();
    }
    
    // Capitalize first letter of regular words
    return word.charAt(0).toUpperCase() + word.slice(1).toLowerCase();
  }).join(' ');
  
  return name;
}


/**
 * Display name that includes the size variant, so two models from the same
 * family (e.g. Gemma 4 E2B vs E4B) are distinguishable in pickers. Matches
 * the downloaded file against the catalog; falls back to filename formatting.
 */
export function richModelName(filename: string): string {
  for (const family of modelFamilies) {
    const variant = family.variants.find((v) => v.filename === filename);
    if (variant) return `${family.name} ${variant.parameterCount}`;
  }
  return formatModelDisplayName(filename);
}

/**
 * Short label for an AI card: names the Auto mode, or the pinned model
 * with prefixes and quantization noise stripped.
 */
export function formatModelForCard(model: string | undefined | null): string {
  if (!model || model === 'auto:offline') return 'Auto - Offline Only';
  if (model === 'auto:my-hardware') return 'Auto - My Hardware';
  if (model === 'auto:online-offline') return 'Auto - Online and Offline';
  if (model.startsWith('online:')) return formatModelDisplayName(model.slice(7));
  if (model.startsWith('external:')) return formatModelDisplayName(model.slice(9));
  return richModelName(model);
}
