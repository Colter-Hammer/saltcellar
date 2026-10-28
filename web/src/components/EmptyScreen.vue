<script setup lang="ts">
// A screen that doesn't exist yet: each milestone 1 issue replaces one.
defineProps<{ message?: string }>()
</script>

<template>
  <p class="p-4 text-sm text-muted-foreground lg:px-6">{{ message ?? 'Nothing here yet.' }}</p>
</template>
