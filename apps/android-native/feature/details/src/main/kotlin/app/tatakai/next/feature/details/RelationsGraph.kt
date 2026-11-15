// SPDX-License-Identifier: AGPL-3.0-or-later
package app.tatakai.next.feature.details

import androidx.compose.animation.core.animate
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.systemBarsPadding
import androidx.compose.foundation.layout.wrapContentSize
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.Fullscreen
import androidx.compose.material3.IconButton
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
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
private val PAD = 16.dp

/**
 * Gesture policy of the graph (2026-11 UI refresh fix): the inline graph inside the Details scroll takes NO drag / pinch gestures, so
 * a one-finger swipe always scrolls the page; it shows the fitted graph, node taps open titles, and the expand button or a tap on
 * the empty canvas opens the fullscreen graph, where one-finger pan and pinch zoom work.
 */
enum class GraphGestures { TAPS_ONLY, PAN_AND_ZOOM }

fun graphGesturesFor(fullscreen: Boolean): GraphGestures = if (fullscreen) GraphGestures.PAN_AND_ZOOM else GraphGestures.TAPS_ONLY

/** Pure viewport math (px), unit-tested. */
object GraphViewport {
    const val MIN_SCALE = 0.4f
    const val MAX_SCALE = 2.5f

    /** Smaller axis does not overflow: centred; bigger axis: kept inside [view - content, 0]. */
    fun clamp(x: Float, y: Float, scale: Float, contentW: Float, contentH: Float, viewW: Float, viewH: Float): Pair<Float, Float> {
        val cw = contentW * scale
        val ch = contentH * scale
        return (if (cw <= viewW) (viewW - cw) / 2f else x.coerceIn(viewW - cw, 0f)) to
            (if (ch <= viewH) (viewH - ch) / 2f else y.coerceIn(viewH - ch, 0f))
    }

    /** Scale that shows the whole graph (never above 100 %, never below [min]). */
    fun fitScale(contentW: Float, contentH: Float, viewW: Float, viewH: Float, min: Float = MIN_SCALE): Float =
        minOf(viewW / contentW, viewH / contentH, 1f).coerceIn(min, MAX_SCALE)

    /** Offset that centres the x [focusX] (content px at scale 1) horizontally, clamped. */
    fun centredOn(focusX: Float, scale: Float, contentW: Float, contentH: Float, viewW: Float, viewH: Float): Pair<Float, Float> =
        clamp(viewW / 2f - focusX * scale, 0f, scale, contentW, contentH, viewW, viewH)
}

private fun contentWidth(layout: RelationsGraphLayout): Dp = NODE_W * layout.cols + GAP_X * (layout.cols - 1) + PAD * 2
private fun contentHeight(layout: RelationsGraphLayout): Dp = NODE_H * layout.rows + GAP_Y * (layout.rows - 1) + PAD * 2
private fun currentCentreX(layout: RelationsGraphLayout): Dp =
    layout.nodes.firstOrNull { it.current }?.let { PAD + (NODE_W + GAP_X) * it.col + NODE_W / 2 } ?: 0.dp

/**
 * Inline graph of the Related tab: fitted (the open title centred when even the fitted graph is wider than the card), taps only.
 * Expand button / empty-canvas tap -> [RelationsGraphFullscreen].
 */
@Composable
internal fun RelationsGraphView(
    layout: RelationsGraphLayout,
    language: TitleLanguage,
    onOpen: (Media) -> Unit,
    modifier: Modifier = Modifier,
) {
    val density = LocalDensity.current
    var fullscreen by rememberSaveable { mutableStateOf(false) }
    BoxWithConstraints(modifier.fillMaxWidth().height(VIEW_H).padding(horizontal = 16.dp)) {
        val (scale, offset) = with(density) {
            val cw = contentWidth(layout).toPx()
            val ch = contentHeight(layout).toPx()
            val vw = maxWidth.toPx()
            val vh = VIEW_H.toPx()
            val s = GraphViewport.fitScale(cw, ch, vw, vh, min = INLINE_MIN_SCALE)
            val (x, y) = GraphViewport.centredOn(currentCentreX(layout).toPx(), s, cw, ch, vw, vh)
            s to Offset(x, y)
        }
        val expand = stringResource(R.string.details_graph_expand)
        Surface(
            color = MaterialTheme.colorScheme.surfaceContainerLow,
            shape = MaterialTheme.shapes.large,
            modifier = Modifier.fillMaxWidth().height(VIEW_H),
        ) {
            // No drag / transform detector here (graphGesturesFor(false) = TAPS_ONLY): one-finger swipes scroll the page. A tap the
            // nodes did not consume (empty canvas) opens the fullscreen graph.
            Box(
                Modifier
                    .clipToBounds()
                    .pointerInput(Unit) { detectTapGestures { fullscreen = true } }
                    .semantics { onClick(label = expand) { fullscreen = true; true } },
            ) {
                GraphContent(layout, language, onOpen, scale, offset)
            }
        }
        FilledTonalIconButton(
            onClick = { fullscreen = true },
            modifier = Modifier.align(Alignment.BottomEnd).padding(8.dp).size(40.dp),
        ) {
            Icon(Icons.Filled.Fullscreen, contentDescription = expand)
        }
    }
    if (fullscreen) {
        RelationsGraphFullscreen(
            layout,
            language,
            onOpen = { media ->
                fullscreen = false
                onOpen(media)
            },
            onDismiss = { fullscreen = false },
        )
    }
}

/** Fullscreen graph (dialog): one-finger pan + pinch zoom, zoom / fit buttons, close button; Back closes it. */
@Composable
private fun RelationsGraphFullscreen(
    layout: RelationsGraphLayout,
    language: TitleLanguage,
    onOpen: (Media) -> Unit,
    onDismiss: () -> Unit,
) {
    Dialog(
        onDismissRequest = onDismiss,
        properties = DialogProperties(usePlatformDefaultWidth = false, dismissOnBackPress = true, dismissOnClickOutside = false),
    ) {
        Surface(color = MaterialTheme.colorScheme.surface, modifier = Modifier.fillMaxSize()) {
            Column(Modifier.fillMaxSize().systemBarsPadding()) {
                Row(Modifier.fillMaxWidth().padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    IconButton(onClick = onDismiss) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.details_graph_close))
                    }
                    Text(stringResource(R.string.details_related), style = MaterialTheme.typography.titleLarge)
                }
                BoxWithConstraints(Modifier.fillMaxWidth().weight(1f)) {
                    PannableGraph(layout, language, onOpen, maxWidth, maxHeight)
                }
            }
        }
    }
}

@Composable
private fun PannableGraph(layout: RelationsGraphLayout, language: TitleLanguage, onOpen: (Media) -> Unit, viewWDp: Dp, viewHDp: Dp) {
    val density = LocalDensity.current
    val spatial = spatialMotion()
    val scope = rememberCoroutineScope()
    val cw: Float
    val ch: Float
    val vw: Float
    val vh: Float
    with(density) {
        cw = contentWidth(layout).toPx()
        ch = contentHeight(layout).toPx()
        vw = viewWDp.toPx()
        vh = viewHDp.toPx()
    }
    fun clamp(o: Offset, s: Float): Offset = GraphViewport.clamp(o.x, o.y, s, cw, ch, vw, vh).let { Offset(it.first, it.second) }
    var scale by remember { mutableFloatStateOf(1f) }
    var offset by remember { mutableStateOf(Offset.Zero) }
    LaunchedEffect(layout, vw, vh) {
        scale = 1f
        offset = with(density) { GraphViewport.centredOn(currentCentreX(layout).toPx(), 1f, cw, ch, vw, vh) }.let { Offset(it.first, it.second) }
    }
    fun animateTo(target: Float) {
        val focus = Offset(vw / 2f, vh / 2f)
        val from = scale
        val start = offset
        val to = target.coerceIn(GraphViewport.MIN_SCALE, GraphViewport.MAX_SCALE)
        if (!spatial) {
            offset = clamp((start - focus) * (to / from) + focus, to)
            scale = to
            return
        }
        scope.launch {
            animate(from, to) { v, _ ->
                offset = clamp((start - focus) * (v / from) + focus, v)
                scale = v
            }
        }
    }
    // graphGesturesFor(true) = PAN_AND_ZOOM: one-finger pan and pinch zoom.
    val transform = rememberTransformableState { zoom, pan, _ ->
        val next = (scale * zoom).coerceIn(GraphViewport.MIN_SCALE, GraphViewport.MAX_SCALE)
        offset = clamp(offset + pan, next)
        scale = next
    }
    Box(Modifier.fillMaxSize()) {
        Box(Modifier.fillMaxSize().clipToBounds().transformable(transform)) {
            GraphContent(layout, language, onOpen, scale, offset)
        }
        Column(Modifier.align(Alignment.BottomEnd).navigationBarsPadding().padding(16.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            FilledTonalIconButton(onClick = { animateTo(scale * 1.3f) }) {
                Icon(Icons.Filled.Add, contentDescription = stringResource(R.string.details_graph_zoom_in))
            }
            FilledTonalIconButton(onClick = { animateTo(scale / 1.3f) }) {
                Icon(Icons.Filled.Remove, contentDescription = stringResource(R.string.details_graph_zoom_out))
            }
            FilledTonalIconButton(onClick = {
                val s = GraphViewport.fitScale(cw, ch, vw, vh)
                scale = s
                offset = clamp(Offset.Zero, s)
            }) {
                Icon(Icons.Filled.FitScreen, contentDescription = stringResource(R.string.details_graph_fit))
            }
        }
    }
}

/** The nodes and edges at [scale] / [offset] (one graphicsLayer: moving never relayouts; layer transforms also apply to taps). */
@Composable
private fun GraphContent(layout: RelationsGraphLayout, language: TitleLanguage, onOpen: (Media) -> Unit, scale: Float, offset: Offset) {
    Box(
        Modifier
            .wrapContentSize(Alignment.TopStart, unbounded = true)
            .requiredSize(contentWidth(layout), contentHeight(layout))
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
                Modifier.offset(x = PAD + (NODE_W + GAP_X) * node.col, y = PAD + (NODE_H + GAP_Y) * node.row),
            )
        }
    }
}

private const val INLINE_MIN_SCALE = 0.3f

@Composable
private fun GraphEdges(layout: RelationsGraphLayout) {
    val main = MaterialTheme.colorScheme.primary
    val other = MaterialTheme.colorScheme.outline
    val byKey = remember(layout) { layout.nodes.associateBy { it.key } }
    Canvas(Modifier.requiredSize(contentWidth(layout), contentHeight(layout))) {
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
