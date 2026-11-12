<script setup lang="ts">
// The way out of a broken bank link (docs/spec/m2-bank-data.md#errors): **Fix at SimpleFIN ↗**
// opens the Bridge to re-authorize the bank, then **Done, sync now** syncs at once, skipping the
// cooldown (it still counts against the request cap).
import { useSyncAfterReconnect } from '@/api/bank'
import { SIMPLEFIN_FIX_URL } from '@/lib/sync'

const sync = useSyncAfterReconnect()
</script>

<template>
  <div class="flex flex-col items-start gap-2">
    <div class="flex flex-wrap gap-2">
      <a
        :href="SIMPLEFIN_FIX_URL"
        target="_blank"
        rel="noopener noreferrer"
        class="inline-flex items-center rounded-md border px-3 py-2"
      >
        Fix at SimpleFIN ↗
      </a>
      <button
        type="button"
        class="rounded-md border px-3 py-2 disabled:opacity-50"
        :disabled="sync.isPending.value"
        @click="sync.mutate()"
      >
        {{ sync.isPending.value ? 'Syncing…' : 'Done, sync now' }}
      </button>
    </div>
    <p v-if="sync.isError.value" role="alert" class="text-sm text-destructive">
      {{ sync.error.value?.message }}
    </p>
  </div>
</template>
