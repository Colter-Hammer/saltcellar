<script setup lang="ts">
// Every amount on screen (docs/spec/visual-system.md#money): tabular figures, font-medium, a real
// minus sign. `signed` is a transaction's amount: money in gets a plus and --money-in, the plus
// being the word beside the color; money out stays plain foreground. `whole` is a headline
// figure, rounded to whole units, with the exact amount as its title and on a tap *(agent)*.
import { computed, ref } from 'vue'
import { formatMoney, useFormatting } from '@/lib/format'

const props = withDefaults(
  defineProps<{
    /** The amount in integer minor units. */
    minor: number
    signed?: boolean
    whole?: boolean
  }>(),
  { signed: false, whole: false },
)

const formatting = useFormatting()
const exactShown = ref(false)
const exact = computed(() => formatMoney(props.minor, formatting.value, { signed: props.signed }))
const shown = computed(() =>
  props.whole && !exactShown.value
    ? formatMoney(props.minor, formatting.value, { signed: props.signed, whole: true })
    : exact.value,
)
const tone = computed(() => props.signed && props.minor > 0 && 'text-money-in')
</script>

<template>
  <button
    v-if="whole"
    type="button"
    :title="exact"
    :aria-label="exact"
    class="font-medium whitespace-nowrap tabular-nums"
    :class="tone"
    @click="exactShown = !exactShown"
  >
    {{ shown }}
  </button>
  <span v-else class="font-medium whitespace-nowrap tabular-nums" :class="tone">{{ shown }}</span>
</template>
