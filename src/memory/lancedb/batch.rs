use super::{LanceDbMemory, PrivacyLevel, ProjectionEntry, StoredRow};

use arrow_array::builder::{FixedSizeListBuilder, Float32Builder};
use arrow_array::{Array, Float64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, SchemaRef};

use anyhow::Context;
use std::sync::Arc;

pub(super) fn build_row_batch(
    schema: SchemaRef,
    row: &StoredRow,
    embedding: Option<&[f32]>,
) -> anyhow::Result<RecordBatch> {
    let id = Arc::new(StringArray::from(vec![Some(row.id.as_str())]));
    let key = Arc::new(StringArray::from(vec![Some(row.key.as_str())]));
    let content = Arc::new(StringArray::from(vec![Some(row.content.as_str())]));
    let category = Arc::new(StringArray::from(vec![Some(row.category.as_str())]));
    let source = Arc::new(StringArray::from(vec![Some(row.source.as_str())]));
    let confidence = Arc::new(Float64Array::from(vec![row.confidence]));
    let importance = Arc::new(Float64Array::from(vec![row.importance]));
    let privacy_level = Arc::new(StringArray::from(vec![Some(row.privacy_level.as_str())]));
    let occurred_at = Arc::new(StringArray::from(vec![Some(row.occurred_at.as_str())]));
    let layer = Arc::new(StringArray::from(vec![Some(row.layer.as_str())]));
    let provenance_source_class = Arc::new(StringArray::from(vec![
        row.provenance_source_class.as_deref(),
    ]));
    let provenance_reference =
        Arc::new(StringArray::from(vec![row.provenance_reference.as_deref()]));
    let provenance_evidence_uri = Arc::new(StringArray::from(vec![
        row.provenance_evidence_uri.as_deref(),
    ]));
    let created_at = Arc::new(StringArray::from(vec![Some(row.created_at.as_str())]));
    let updated_at = Arc::new(StringArray::from(vec![Some(row.updated_at.as_str())]));
    let status = Arc::new(StringArray::from(vec![Some(row.embedding_status.as_str())]));

    let dims = match schema.field_with_name("embedding")?.data_type() {
        DataType::FixedSizeList(_, n) => *n,
        other => anyhow::bail!("Unexpected embedding type in schema: {other:?}"),
    };

    let dims_usize = usize::try_from(dims)
        .with_context(|| format!("Invalid embedding dimension in schema: {dims}"))?;

    let mut emb_builder = FixedSizeListBuilder::new(Float32Builder::new(), dims);
    if let Some(v) = embedding {
        if v.len() != dims_usize {
            anyhow::bail!(
                "Embedding dimension mismatch: got {}, expected {}",
                v.len(),
                dims
            );
        }
        emb_builder.values().append_slice(v);
        emb_builder.append(true);
    } else {
        for _ in 0..dims_usize {
            emb_builder.values().append_value(0.0);
        }
        emb_builder.append(false);
    }
    let embedding_arr = Arc::new(emb_builder.finish());

    let cols: Vec<Arc<dyn Array>> = vec![
        id,
        key,
        content,
        category,
        source,
        confidence,
        importance,
        privacy_level,
        occurred_at,
        layer,
        provenance_source_class,
        provenance_reference,
        provenance_evidence_uri,
        created_at,
        updated_at,
        embedding_arr,
        status,
    ];
    Ok(RecordBatch::try_new(schema, cols)?)
}

#[allow(clippy::too_many_lines)]
pub(super) fn parse_rows(batch: &RecordBatch) -> Vec<StoredRow> {
    let id = batch
        .column_by_name("id")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let key = batch
        .column_by_name("key")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let content = batch
        .column_by_name("content")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let category = batch
        .column_by_name("category")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let source = batch
        .column_by_name("source")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let confidence = batch
        .column_by_name("confidence")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let importance = batch
        .column_by_name("importance")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let privacy_level = batch
        .column_by_name("privacy_level")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let occurred_at = batch
        .column_by_name("occurred_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let layer = batch
        .column_by_name("layer")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let provenance_source_class = batch
        .column_by_name("provenance_source_class")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let provenance_reference = batch
        .column_by_name("provenance_reference")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let provenance_evidence_uri = batch
        .column_by_name("provenance_evidence_uri")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let created_at = batch
        .column_by_name("created_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let updated_at = batch
        .column_by_name("updated_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let embedding_status = batch
        .column_by_name("embedding_status")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let (
        Some(id),
        Some(key),
        Some(content),
        Some(category),
        Some(created_at),
        Some(updated_at),
        Some(embedding_status),
    ) = (
        id,
        key,
        content,
        category,
        created_at,
        updated_at,
        embedding_status,
    )
    else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(batch.num_rows());
    for i in 0..batch.num_rows() {
        if id.is_null(i)
            || key.is_null(i)
            || content.is_null(i)
            || category.is_null(i)
            || created_at.is_null(i)
            || updated_at.is_null(i)
            || embedding_status.is_null(i)
        {
            continue;
        }

        out.push(StoredRow {
            id: id.value(i).to_string(),
            key: key.value(i).to_string(),
            content: content.value(i).to_string(),
            category: category.value(i).to_string(),
            source: source
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| {
                    LanceDbMemory::source_to_str(&LanceDbMemory::source_from_category(
                        &LanceDbMemory::str_to_category(category.value(i)),
                    ))
                    .to_string()
                }),
            confidence: confidence.map_or(0.0, |col| col.value(i)),
            importance: importance.map_or(0.0, |col| col.value(i)),
            privacy_level: privacy_level
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| "private".to_string()),
            occurred_at: occurred_at
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| created_at.value(i).to_string()),
            layer: layer
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| "working".to_string()),
            provenance_source_class: provenance_source_class
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string())),
            provenance_reference: provenance_reference
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string())),
            provenance_evidence_uri: provenance_evidence_uri
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string())),
            created_at: created_at.value(i).to_string(),
            updated_at: updated_at.value(i).to_string(),
            embedding_status: embedding_status.value(i).to_string(),
        });
    }
    out
}

pub(super) fn parse_entries(batch: &RecordBatch) -> Vec<ProjectionEntry> {
    let id = batch
        .column_by_name("id")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let key = batch
        .column_by_name("key")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let content = batch
        .column_by_name("content")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let category = batch
        .column_by_name("category")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let source = batch
        .column_by_name("source")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let confidence = batch
        .column_by_name("confidence")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let importance = batch
        .column_by_name("importance")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let privacy_level = batch
        .column_by_name("privacy_level")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let occurred_at = batch
        .column_by_name("occurred_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let created_at = batch
        .column_by_name("created_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let (Some(id), Some(key), Some(content), Some(category), Some(created_at)) =
        (id, key, content, category, created_at)
    else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(batch.num_rows());
    for i in 0..batch.num_rows() {
        if id.is_null(i)
            || key.is_null(i)
            || content.is_null(i)
            || category.is_null(i)
            || created_at.is_null(i)
        {
            continue;
        }

        let parsed_category = LanceDbMemory::str_to_category(category.value(i));
        let parsed_source = source
            .and_then(|col| (!col.is_null(i)).then(|| LanceDbMemory::str_to_source(col.value(i))))
            .unwrap_or_else(|| LanceDbMemory::source_from_category(&parsed_category));

        out.push(ProjectionEntry {
            id: id.value(i).to_string(),
            key: key.value(i).to_string(),
            content: content.value(i).to_string(),
            category: parsed_category,
            timestamp: created_at.value(i).to_string(),
            source: parsed_source,
            confidence: confidence.map_or(0.0, |col| col.value(i)),
            importance: importance.map_or(0.0, |col| col.value(i)),
            privacy_level: privacy_level
                .and_then(|col| {
                    (!col.is_null(i)).then(|| LanceDbMemory::str_to_privacy(col.value(i)))
                })
                .unwrap_or(PrivacyLevel::Private),
            occurred_at: occurred_at
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| created_at.value(i).to_string()),
            score: None,
        });
    }
    out
}

#[allow(clippy::cast_possible_truncation)]
pub(super) fn parse_entries_and_score(
    batch: &RecordBatch,
    score_col: &str,
) -> (Vec<ProjectionEntry>, Vec<f32>) {
    let id = batch
        .column_by_name("id")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let key = batch
        .column_by_name("key")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let content = batch
        .column_by_name("content")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let category = batch
        .column_by_name("category")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let source = batch
        .column_by_name("source")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let confidence = batch
        .column_by_name("confidence")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let importance = batch
        .column_by_name("importance")
        .and_then(|c| c.as_any().downcast_ref::<Float64Array>());
    let privacy_level = batch
        .column_by_name("privacy_level")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let occurred_at = batch
        .column_by_name("occurred_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());
    let created_at = batch
        .column_by_name("created_at")
        .and_then(|c| c.as_any().downcast_ref::<StringArray>());

    let (Some(id), Some(key), Some(content), Some(category), Some(created_at)) =
        (id, key, content, category, created_at)
    else {
        return (Vec::new(), Vec::new());
    };

    let col = batch.column_by_name(score_col);
    let f32s = col.and_then(|c| c.as_any().downcast_ref::<arrow_array::Float32Array>());
    let f64s = col.and_then(|c| c.as_any().downcast_ref::<arrow_array::Float64Array>());

    let mut entries_out = Vec::with_capacity(batch.num_rows());
    let mut scores_out = Vec::with_capacity(batch.num_rows());

    for i in 0..batch.num_rows() {
        if id.is_null(i)
            || key.is_null(i)
            || content.is_null(i)
            || category.is_null(i)
            || created_at.is_null(i)
        {
            continue;
        }

        let score = if let Some(a) = f32s {
            a.value(i)
        } else if let Some(a) = f64s {
            a.value(i) as f32
        } else {
            0.0
        };

        let parsed_category = LanceDbMemory::str_to_category(category.value(i));
        let parsed_source = source
            .and_then(|col| (!col.is_null(i)).then(|| LanceDbMemory::str_to_source(col.value(i))))
            .unwrap_or_else(|| LanceDbMemory::source_from_category(&parsed_category));

        entries_out.push(ProjectionEntry {
            id: id.value(i).to_string(),
            key: key.value(i).to_string(),
            content: content.value(i).to_string(),
            category: parsed_category,
            timestamp: created_at.value(i).to_string(),
            source: parsed_source,
            confidence: confidence.map_or(0.0, |col| col.value(i)),
            importance: importance.map_or(0.0, |col| col.value(i)),
            privacy_level: privacy_level
                .and_then(|col| {
                    (!col.is_null(i)).then(|| LanceDbMemory::str_to_privacy(col.value(i)))
                })
                .unwrap_or(PrivacyLevel::Private),
            occurred_at: occurred_at
                .and_then(|col| (!col.is_null(i)).then(|| col.value(i).to_string()))
                .unwrap_or_else(|| created_at.value(i).to_string()),
            score: None,
        });
        scores_out.push(score);
    }

    (entries_out, scores_out)
}
