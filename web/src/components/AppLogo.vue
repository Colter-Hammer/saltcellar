<script setup lang="ts">
// The mark in the palette accent beside the "Saltcellar" wordmark, set as text
// (docs/spec/visual-system.md#logo). The mark is src/assets/logo/mark.svg, drawn inline so its
// currentColor takes the accent: swapping that file swaps every logo in the app. Sized by the text
// it sits in.
import mark from '@/assets/logo/mark.svg?raw'
</script>

<template>
  <span class="inline-flex items-center gap-[0.35em] font-semibold tracking-tight">
    <!-- eslint-disable vue/no-v-html -- our own asset, bundled at build time -->
    <span
      class="inline-block size-[1.5em] shrink-0 text-primary [&>svg]:size-full"
      aria-hidden="true"
      data-testid="logo-mark"
      v-html="mark"
    />
    <!-- eslint-enable vue/no-v-html -->
    <span>Saltcellar</span>
  </span>
</template>
