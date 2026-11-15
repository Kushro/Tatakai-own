// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import androidx.compose.animation.core.animate
import androidx.compose.foundation.BorderStroke
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.rememberTransformableState
import androidx.compose.foundation.gestures.transformable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Add
import androidx.compose.material.icons.filled.FitScreen
import androidx.compose.material.icons.filled.Remove
import androidx.compose.material3.FilledTonalIconButton
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import app.tatakai.next.model.Media
import app.tatakai.next.model.MediaRelation
import app.tatakai.next.model.MediaRef
import app.tatakai.next.model.TitleLanguage
import app.tatakai.next.ui.look.spatialMotion
import app.tatakai.next.ui.media.MediaCover
import app.tatakai.next.ui.media.displayTitle
import kotlinx.coroutines.launch

// 2026-11 UI refresh: Details › Relacionados as a franchise graph (desktop relations graph). The main chain (watch order, or
// prequel → this → sequel) runs horizontally in the primary color; side stories, spin-offs, adaptations... hang below the open title.
// Built only from the relations / seasons Details already loads (no extra AniList query). Pan and pinch zoom with `transformable`,
// buttons for zoom and fit; the transform lives in one graphicsLayer (no relayout while moving).

/** One node of the laid out graph; [col] / [row] are grid cells (row 0 = main chain). */
data class GraphNode(
    val key: String,
    val media: Media,
    /** AniList relation to the open title (null for the open title and for chain entries known only from the watch order). */
    val relation: String?,
    val col: Int,
    val row: Int,
    val main: Boolean,
    val current: Boolean,
)

data class GraphEdge(val from: String, val to: String, val main: Boolean)

data class RelationsGraphLayout(val nodes: List<GraphNode>, val edges: List<GraphEdge>, val cols: Int, val rows: Int)

private fun refKeyOf(ref: MediaRef, fallback: String): String =
    ref.anilistId?.let { "a$it" } ?: ref.malId?.let { "m$it" } ?: "t:$fallback"

private fun sameMedia(a: MediaRef, b: MediaRef): Boolean =
    (a.anilistId != null && a.anilistId == b.anilistId) || (a.malId != null && a.malId == b.malId)

/**
 * Pure layout (unit-tested). Main chain = [seasons] when it holds the open title (the watch order), else the PREQUEL relations, the
 * open title and the SEQUEL relations. Every other relation goes in rows under the chain, centred on the open title, at most
 * [perRow] per row, in the classic relation order. Edges: consecutive chain entries (main) and open title → each other node.
 */
fun layoutRelationsGraph(
    current: Media,
    seasons: List<Media>,
    relations: List<MediaRelation>,
    perRow: Int = 4,
): RelationsGraphLayout {
    val sorted = sortRelations(relations)
    val relationOf = { m: Media -> sorted.firstOrNull { sameMedia(it.media.ref, m.ref) }?.relation }
    val chain: List<Media> = if (seasons.size > 1 && seasons.any { sameMedia(it.ref, current.ref) }) {
        seasons.map { s -> if (sameMedia(s.ref, current.ref)) current else s }
    } else {
        sorted.filter { it.relation == "PREQUEL" }.map { it.media } + current + sorted.filter { it.relation == "SEQUEL" }.map { it.media }
    }
    val chainNodes = chain.mapIndexed { i, m ->
        val isCurrent = sameMedia(m.ref, current.ref) || m === current
        GraphNode(refKeyOf(m.ref, m.title), m, if (isCurrent) null else relationOf(m), col = i, row = 0, main = true, current = isCurrent)
    }.distinctBy { it.key }
    val currentNode = chainNodes.first { it.current }
    val inChain = chainNodes.map { it.key }.toSet()
    val others = sorted.filter { refKeyOf(it.media.ref, it.media.title) !in inChain }.distinctBy { refKeyOf(it.media.ref, it.media.title) }
    val per = perRow.coerceAtLeast(1)
    val otherNodes = others.mapIndexed { i, rel ->
        val row = 1 + i / per
        val inRow = minOf(per, others.size - (row - 1) * per)
        val start = (currentNode.col - (inRow - 1) / 2).coerceAtLeast(0)
        GraphNode(refKeyOf(rel.media.ref, rel.media.title), rel.media, rel.relation, col = start + i % per, row = row, main = false, current = false)
    }
    val nodes = chainNodes + otherNodes
    val edges = chainNodes.zipWithNext { a, b -> GraphEdge(a.key, b.key, main = true) } +
        otherNodes.map { GraphEdge(currentNode.key, it.key, main = false) }
    return RelationsGraphLayout(
        nodes = nodes,
        edges = edges,
        cols = (nodes.maxOfOrNull { it.col } ?: 0) + 1,
        rows = (nodes.maxOfOrNull { it.row } ?: 0) + 1,
    )
}

private val NODE_W = 112.dp
private val NODE_H = 212.dp
private val GAP_X = 36.dp
private val GAP_Y = 40.dp
private val VIEW_H = 340.dp
private const val MIN_SCALE = 0.4f
private const val MAX_SCALE = 2.5f

@Composable
internal fun RelationsGraphView(
    layout: RelationsGraphLayout,
    language: TitleLanguage,
    onOpen: (Media) -> Unit,
    modifier: Modifier = Modifier,
) {
    val density = LocalDensity.current
    val contentW = NODE_W * layout.cols + GAP_X * (layout.cols - 1) + 32.dp
    val contentH = NODE_H * layout.rows + GAP_Y * (layout.rows - 1) + 32.dp
    val spatial = spatialMotion()
    val scope = rememberCoroutineScope()
    BoxWithConstraints(
        modifier
            .fillMaxWidth()
            .height(VIEW_H)
            .padding(horizontal = 16.dp),
    ) {
        val viewW = maxWidth
        var scale by remember { mutableFloatStateOf(1f) }
        var offset by remember { mutableStateOf(Offset.Zero) }
        fun clampOffset(o: Offset, s: Float): Offset = with(density) {
            val cw = contentW.toPx() * s
            val ch = contentH.toPx() * s
            val vw = viewW.toPx()
            val vh = VIEW_H.toPx()
            Offset(
                if (cw <= vw) (vw - cw) / 2f else o.x.coerceIn(vw - cw, 0f),
                if (ch <= vh) (vh - ch) / 2f else o.y.coerceIn(vh - ch, 0f),
            )
        }
        // Start at 100 % with the open title centred.
        fun centred(): Offset = with(density) {
            val cur = layout.nodes.firstOrNull { it.current } ?: return@with Offset.Zero
            val x = (16.dp + (NODE_W + GAP_X) * cur.col + NODE_W / 2).toPx()
            clampOffset(Offset(viewW.toPx() / 2f - x, 0f), 1f)
        }
        fun fitScale(): Float = with(density) {
            minOf(viewW.toPx() / contentW.toPx(), VIEW_H.toPx() / contentH.toPx(), 1f).coerceIn(MIN_SCALE, MAX_SCALE)
        }
        LaunchedEffect(layout, viewW) {
            scale = 1f
            offset = centred()
        }
        fun animateTo(targetScale: Float, focus: Offset = with(density) { Offset(viewW.toPx() / 2f, VIEW_H.toPx() / 2f) }) {
            val from = scale
            val startOffset = offset
            val to = targetScale.coerceIn(MIN_SCALE, MAX_SCALE)
            if (!spatial) {
                offset = clampOffset((startOffset - focus) * (to / from) + focus, to)
                scale = to
                return
            }
            scope.launch {
                animate(from, to) { v, _ ->
                    offset = clampOffset((startOffset - focus) * (v / from) + focus, v)
                    scale = v
                }
            }
        }
        val transform = rememberTransformableState { zoom, pan, _ ->
            val next = (scale * zoom).coerceIn(MIN_SCALE, MAX_SCALE)
            offset = clampOffset((offset + pan), next)
            scale = next
        }
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().height(VIEW_H),
        ) {
            Box(Modifier.clipToBounds().transformable(transform)) {
                Box(
                    Modifier
                        .requiredSize(contentW, contentH)
                        .align(Alignment.TopStart)
                        .graphicsLayer {
                            transformOrigin = TransformOrigin(0f, 0f)
                            translationX = offset.x
                            translationY = offset.y
                            scaleX = scale
                            scaleY = scale
                        },
                ) {
                    GraphEdges(layout)
                    layout.nodes.forEach { node ->
                        GraphNodeCard(
                            node,
                            language,
                            onOpen,
                            Modifier.offset(x = 16.dp + (NODE_W + GAP_X) * node.col, y = 16.dp + (NODE_H + GAP_Y) * node.row),
                        )
                    }
                }
            }
        }
        Column(Modifier.align(Alignment.BottomEnd).padding(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            FilledTonalIconButton(onClick = { animateTo(scale * 1.3f) }, modifier = Modifier.size(40.dp)) {
                Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.details_graph_zoom_in))
            }
            FilledTonalIconButton(onClick = { animateTo(scale / 1.3f) }, modifier = Modifier.size(40.dp)) {
                Icon(Icons.Filled.Remove, contentDescription = stringResource(R.string.details_graph_zoom_out))
            }
            FilledTonalIconButton(
                onClick = {
                    val s = fitScale()
                    scale = s
                    offset = clampOffset(Offset.Zero, s)
                },
                modifier = Modifier.size(40.dp),
            ) {
                Icon(Icons.Filled.FitScreen, contentDescription = stringResource(R.string.details_graph_fit))
            }
        }
    }
}

@Composable
private fun GraphEdges(layout: RelationsGraphLayout) {
    val main = MaterialTheme.colorScheme.primary
    val other = MaterialTheme.colorScheme.outline
    val byKey = remember(layout) { layout.nodes.associateBy { it.key } }
    Canvas(Modifier.requiredSize(NODE_W * layout.cols + GAP_X * (layout.cols - 1) + 32.dp, NODE_H * layout.rows + GAP_Y * (layout.rows - 1) + 32.dp)) {
        fun anchor(n: GraphNode, side: Int): Offset {
            // side: 0 = right middle of the cover, 1 = left middle, 2 = bottom centre, 3 = top centre.
            val x = (16.dp + (NODE_W + GAP_X) * n.col).toPx()
            val y = (16.dp + (NODE_H + GAP_Y) * n.row).toPx()
            val w = NODE_W.toPx()
            val coverH = NODE_W.toPx() / COVER
            return when (side) {
                0 -> Offset(x + w, y + coverH / 2f)
                1 -> Offset(x, y + coverH / 2f)
                2 -> Offset(x + w / 2f, y + NODE_H.toPx() - 8.dp.toPx())
                else -> Offset(x + w / 2f, y)
            }
        }
        layout.edges.forEach { e ->
            val a = byKey[e.from] ?: return@forEach
            val b = byKey[e.to] ?: return@forEach
            if (e.main) {
                drawLine(main, anchor(a, 0), anchor(b, 1), strokeWidth = 4.dp.toPx())
            } else {
                val start = anchor(a, 2)
                val end = anchor(b, 3)
                val midY = (start.y + end.y) / 2f
                val dash = PathEffect.dashPathEffect(floatArrayOf(10f, 8f))
                drawLine(other, start, Offset(start.x, midY), strokeWidth = 2.dp.toPx(), pathEffect = dash)
                drawLine(other, Offset(start.x, midY), Offset(end.x, midY), strokeWidth = 2.dp.toPx(), pathEffect = dash)
                drawLine(other, Offset(end.x, midY), end, strokeWidth = 2.dp.toPx(), pathEffect = dash)
            }
        }
    }
}

private const val COVER = 13f / 18f

@Composable
private fun GraphNodeCard(node: GraphNode, language: TitleLanguage, onOpen: (Media) -> Unit, modifier: Modifier = Modifier) {
    Surface(
        onClick = { if (!node.current) onOpen(node.media) },
        shape = MaterialTheme.shapes.medium,
        color = if (node.current) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceContainerHigh,
        border = if (node.current) BorderStroke(2.dp, MaterialTheme.colorScheme.primary) else null,
        modifier = modifier.width(NODE_W).height(NODE_H),
    ) {
        Column {
            MediaCover(url = node.media.cover, modifier = Modifier.fillMaxWidth())
            Column(Modifier.padding(6.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                Text(
                    node.relation?.let { relationLabel(it) } ?: stringResource(R.string.details_graph_this),
                    style = MaterialTheme.typography.labelSmall,
                    color = if (node.main) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
                Text(
                    node.media.displayTitle(language),
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                    textAlign = TextAlign.Start,
                )
            }
        }
    }
}

/** Width the graph would take at 100 % (used by tests / callers that want to know whether it overflows). */
internal fun graphContentWidth(layout: RelationsGraphLayout): Dp = NODE_W * layout.cols + GAP_X * (layout.cols - 1) + 32.dp
