<script setup lang="ts">
import AppShell from '@/components/AppShell.vue'
</script>

<template>
  <RouterView v-if="$route.meta.bare" />
  <AppShell v-else>
    <RouterView />
  </AppShell>
</template>
