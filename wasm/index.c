typedef uint32_t doc_id_t;

// NOTE: This is used in many places and its order and values are intentional.
typedef enum {
  REQUIRE = 0,
  CONTAIN = 1,
  EXCLUDE = 2,
} mode_t;

// This should be called before every query.
WASM_EXPORT void reset(void) {
  heap = &__heap_base;
}

typedef struct {
  uint32_t first_rank;
  // This is a flattened form of char const*[][].
  // There's a subarray for each mode, and they are ordered according to their numeric value (see mode_t).
  // Each mode contains pointers to byte arrays containing serialised Roaring Bitmaps representing a term.
  // Each mode is terminated by NULL.
  // For example: `{
  //   &bitmapForHello, &bitmapForWorld, NULL,
  //   &bitmapForThe, &bitmapForQuick, &bitmapForFox, NULL,
  //   &bitmapForAstronaut, NULL,
  // }`.
  char const* serialised[MAX_QUERY_TERMS + 3];
} index_query_t;

// Result of a query executed within WASM.
typedef struct {
  // Value to be provided as `index_query_t->first_rank` to get next set of documents. -1 if there are no more documents left.
  int32_t continuation;
  // Total amount of documents matching the search query.
  uint32_t total;
  // How many documents retrieved in this set.
  uint8_t count;
  // IDs of the documents in this set.
  doc_id_t documents[MAX_RESULTS];
} results_t;

uint32_t min(uint32_t a, uint32_t b) {
  return a < b ? a : b;
}

// Function to be called from JS that allocates enough memory for a query and returns the pointer to it.
WASM_EXPORT index_query_t* index_query_malloc(void) {
  return malloc(sizeof(index_query_t));
}

// Internal function used to deserialise multiple bitmaps from a `index_query_t->serialised` value.
// `mode_query_data` should point to `index_query_t->serialised`, and `mode_query_data_next` should be the next offset to process.
// `deserialised_holding` must be provided as a scratch space so that pointers to deserialised bitmaps (which are allocated on the heap) can be stored somewhere temporarily.
// The deserialised bitmaps are then combined using OR on the heap and the pointer to it will be returned. If there are no bitmaps to combine, NULL is returned instead.
inline roaring_bitmap_t* index_deserialise_and_combine(
  roaring_bitmap_t** deserialised_holding,
  char const** mode_query_data,
  size_t* mode_query_data_next
) {
  size_t bitmaps_to_combine_count = 0;
  char const* serialised;
  while ((serialised = mode_query_data[*mode_query_data_next])) {
    deserialised_holding[bitmaps_to_combine_count] = roaring_bitmap_portable_deserialize(serialised);
    bitmaps_to_combine_count++;
    (*mode_query_data_next)++;
  }
  (*mode_query_data_next)++;
  if (!bitmaps_to_combine_count) {
    return NULL;
  }
  return roaring_bitmap_or_many(
    bitmaps_to_combine_count,
    (roaring_bitmap_t const**) deserialised_holding
  );
}

// Function to be called from JS that executes a query. May return NULL if an error occurred.
WASM_EXPORT results_t* index_query(index_query_t* query) {
  roaring_bitmap_t* result_bitmap = NULL;
  size_t i = 0;

  // REQUIRE.
  while (query->serialised[i]) {
    roaring_bitmap_t* bitmap = roaring_bitmap_portable_deserialize(query->serialised[i]);
    if (result_bitmap == NULL) result_bitmap = bitmap;
    else roaring_bitmap_and_inplace(result_bitmap, bitmap);
    i++;
  }
  i++;

  // CONTAIN.
  // Repurpose query data array for storing pointers to deserialised bitmaps.
  roaring_bitmap_t* contain_bitmaps_combined = index_deserialise_and_combine((roaring_bitmap_t**) &query->serialised[i], query->serialised, &i);
  if (contain_bitmaps_combined != NULL) {
    if (result_bitmap == NULL) result_bitmap = contain_bitmaps_combined;
    else roaring_bitmap_and_inplace(result_bitmap, contain_bitmaps_combined);
  }

  // EXCLUDE.
  // Repurpose query data array for storing pointers to deserialised bitmaps.
  roaring_bitmap_t* exclude_bitmaps_combined = index_deserialise_and_combine((roaring_bitmap_t**) &query->serialised[i], query->serialised, &i);
  if (exclude_bitmaps_combined != NULL) {
    if (result_bitmap == NULL) result_bitmap = exclude_bitmaps_combined;
    else roaring_bitmap_andnot_inplace(result_bitmap, exclude_bitmaps_combined);
  }

  if (result_bitmap == NULL) {
    return NULL;
  }

  uint64_t doc_count = roaring_bitmap_get_cardinality(result_bitmap);
  results_t* results = malloc(sizeof(results_t));

  uint32_t first_rank = query->first_rank;

  // TODO Should we worry about this unchecked cast?
  results->total = (uint32_t) doc_count;
  if (first_rank >= doc_count) {
    results->continuation = -1;
    results->count = 0;
  } else {
    uint32_t last_rank = min(doc_count - 1, first_rank + MAX_RESULTS - 1);
    uint32_t count = last_rank + 1 - first_rank;
    roaring_bitmap_range_uint32_array(result_bitmap, first_rank, count, results->documents);
    results->continuation = last_rank == doc_count - 1 ? -1 : last_rank + 1;
    results->count = count;
  }

  return results;
}
