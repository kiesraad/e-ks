/**
 * Holds the state of a single drag interaction and exposes helpers to update it.
 */
export class DragSession {
  readonly target: HTMLTableRowElement;
  readonly startRect: DOMRect;
  readonly startIndex: number;
  readonly startY: number;
  readonly startScrollY: number;
  readonly touchId: number | null;

  boundaryTop = 0;
  boundaryBottom = 0;
  positions: number[] = [];
  liveOrder: HTMLTableRowElement[];
  pendingY: number | null = null;
  animationScheduled = false;
  isDragging = false;

  /**
   * Create a new drag session for a row.
   */
  constructor(
    target: HTMLTableRowElement,
    clientY: number,
    startIndex: number,
    rows: HTMLTableRowElement[],
    touchId: number | null,
  ) {
    this.target = target;
    this.startRect = target.getBoundingClientRect();
    this.startIndex = startIndex;
    this.startY = clientY;
    this.startScrollY = globalThis.scrollY;
    this.liveOrder = rows;
    this.touchId = touchId;
  }

  /**
   * Capture current table geometry for drag calculations.
   */
  setGeometry(rows: HTMLTableRowElement[]) {
    const rects = rows.map((r) => r.getBoundingClientRect());
    const scrollY = globalThis.scrollY;
    this.positions = rects.map((rect) => rect.top + scrollY + rect.height / 2);
    this.boundaryTop = Math.min(...rects.map((rect) => rect.top + scrollY));
    this.boundaryBottom = Math.max(
      ...rects.map((rect) => rect.bottom + scrollY),
    );
  }

  /**
   * Store the latest pointer Y position.
   */
  setPendingY(clientY: number) {
    this.pendingY = clientY;
  }

  getPendingY() {
    return this.pendingY;
  }

  /**
   * Mark an animation frame as scheduled; returns false if one is already pending.
   */
  scheduleAnimation() {
    if (this.animationScheduled) {
      return false;
    }

    this.animationScheduled = true;
    return true;
  }

  /**
   * Reset scheduling flags at the start of a frame.
   */
  startFrame() {
    this.animationScheduled = false;
  }

  /**
   * Note that the drag has moved the row.
   */
  markDragging() {
    this.isDragging = true;
  }

  /**
   * Compute constrained drag distances and current bounds.
   */
  computeDragDelta(pendingY: number) {
    const currentScroll = globalThis.scrollY;
    const startDocY = this.startY + this.startScrollY;
    const currentDocY = pendingY + currentScroll;

    let deltaY = currentDocY - startDocY;
    const startRectTopDoc = this.startRect.top + this.startScrollY;
    const startRectBottomDoc = this.startRect.bottom + this.startScrollY;
    const minDelta = this.boundaryTop - startRectTopDoc;
    const maxDelta = this.boundaryBottom - startRectBottomDoc;
    if (deltaY < minDelta) {
      deltaY = minDelta;
    } else if (deltaY > maxDelta) {
      deltaY = maxDelta;
    }

    const currentTop = startRectTopDoc + deltaY;
    const currentBottom = currentTop + this.startRect.height;
    return { deltaY, currentTop, currentBottom };
  }

  /**
   * Apply transforms to other rows based on the dragged position.
   */
  shiftOtherRows(
    rows: HTMLTableRowElement[],
    currentTop: number,
    currentBottom: number,
  ) {
    const moveUp = `translate(0, ${-this.startRect.height}px)`;
    const moveDown = `translate(0, ${this.startRect.height}px)`;

    const movedAbove = new Set<number>();
    const movedBelow = new Set<number>();

    rows.forEach((row, index) => {
      if (row === this.target) {
        return;
      }

      const originallyBelow = index > this.startIndex;
      const originallyAbove = index < this.startIndex;

      if (originallyBelow && currentBottom > this.positions[index]) {
        row.style.transform = moveUp;
        movedBelow.add(index);
      } else if (originallyAbove && currentTop < this.positions[index]) {
        row.style.transform = moveDown;
        movedAbove.add(index);
      } else {
        row.style.transform = "";
      }
    });

    return { movedAbove, movedBelow };
  }

  /**
   * Recompute the live order of rows while dragging.
   */
  updateLiveOrder(
    rows: HTMLTableRowElement[],
    movedAbove: Set<number>,
    movedBelow: Set<number>,
  ) {
    const dragIndex = this.startIndex - movedAbove.size + movedBelow.size;
    const newOrder: Array<HTMLTableRowElement | null> = new Array(
      rows.length,
    ).fill(null);

    rows.forEach((row, index) => {
      let newIndex = index;

      if (row === this.target) {
        newIndex = dragIndex;
      } else if (index < this.startIndex && movedAbove.has(index)) {
        newIndex = index + 1;
      } else if (index > this.startIndex && movedBelow.has(index)) {
        newIndex = index - 1;
      }

      newOrder[newIndex] = row;
    });

    const compactOrder = newOrder.filter(
      (row): row is HTMLTableRowElement => row !== null,
    );
    this.liveOrder = compactOrder;
    return compactOrder;
  }

  /**
   * Get the current order or fall back to the original rows.
   */
  getOrder(fallback: HTMLTableRowElement[]) {
    return this.liveOrder ?? fallback;
  }

  /**
   * Find the active touch for this drag session.
   */
  getActiveTouch(touches: TouchList): Touch | null {
    if (this.touchId === null) {
      return null;
    }

    return (
      Array.from(touches).find((touch) => touch.identifier === this.touchId) ??
      null
    );
  }
}

/**
 * Enables drag-and-drop reordering for sortable tables in the DOM.
 */
export class SortableTable {
  private readonly tbody: HTMLTableSectionElement;
  private readonly handles: HTMLElement[];
  private readonly positionCellCache = new Map<
    HTMLTableRowElement,
    HTMLTableCellElement
  >();

  private rows: HTMLTableRowElement[];
  private onChange?: (order: string[]) => void;

  private drag: DragSession | null = null;
  private dragStartOrder: string[] | null = null;
  private timeout: number | null = null;

  constructor(tbody: HTMLTableSectionElement) {
    this.tbody = tbody;
    this.rows = Array.from(tbody.querySelectorAll<HTMLTableRowElement>("tr"));
    this.handles = Array.from(
      tbody.querySelectorAll<HTMLElement>("tr td.drag-handle"),
    );
  }

  public setOnChange(callback: (order: string[]) => void) {
    this.onChange = callback;
  }

  /**
   * Bind drag start events to each handle.
   */
  public attachHandleEvents() {
    this.handles.forEach((handle) => {
      const row: HTMLTableRowElement | null = handle.closest("tr");

      if (!row) {
        return;
      }

      handle.addEventListener("mousedown", (event) => {
        event.preventDefault();

        this.reset();
        this.startDrag(row, event.clientY);
      });

      handle.addEventListener(
        "touchstart",
        (event) => {
          if (this.drag) {
            return;
          }

          const touch = event.changedTouches[0];
          if (!touch) {
            return;
          }

          event.preventDefault();

          this.reset();
          this.startDrag(row, touch.clientY, touch.identifier);
        },
        { passive: false },
      );
    });
  }

  /**
   * Bind global move/end/click handlers for drag interactions.
   */
  public attachGlobalEvents() {
    globalThis.addEventListener("mousemove", (event) =>
      this.handleMouseMove(event),
    );
    globalThis.addEventListener("mouseup", (event) =>
      this.handleMouseUp(event),
    );
    globalThis.addEventListener(
      "touchmove",
      (event) => this.handleTouchMove(event),
      { passive: false },
    );
    globalThis.addEventListener("touchend", (event) =>
      this.handleTouchEnd(event),
    );
    globalThis.addEventListener("touchcancel", (event) =>
      this.handleTouchEnd(event),
    );
  }

  /**
   * Initialize a drag session for the given row.
   */
  private startDrag(
    row: HTMLTableRowElement,
    clientY: number,
    touchId: number | null = null,
  ) {
    this.dragStartOrder = this.getRowOrderIds(this.rows);
    this.drag = new DragSession(
      row,
      clientY,
      this.rows.indexOf(row),
      this.rows.slice(),
      touchId,
    );

    row.classList.add("dragging");
    this.computeGeometry();
    this.updatePositionLabels(this.rows);
  }

  /**
   * Update cached geometry for drag calculations.
   */
  private computeGeometry() {
    this.drag?.setGeometry(this.rows);
  }

  /**
   * Queue a drag update for the latest pointer Y position.
   */
  private handlePointerMove(clientY: number) {
    if (!this.drag) {
      return;
    }

    this.drag.setPendingY(clientY);
    this.scheduleDragUpdate();
  }

  /**
   * Mouse move handler forwarding to shared pointer logic.
   */
  private handleMouseMove(event: MouseEvent) {
    this.handlePointerMove(event.clientY);
  }

  /**
   * Touch move handler forwarding to shared pointer logic.
   */
  private handleTouchMove(event: TouchEvent) {
    if (!this.drag) {
      return;
    }

    const touch = this.drag?.getActiveTouch(event.changedTouches);
    if (!touch) {
      return;
    }

    event.preventDefault();
    this.handlePointerMove(touch.clientY);
  }

  /**
   * Mouse up handler that finalises the drag.
   */
  private handleMouseUp(event: MouseEvent) {
    if (!this.drag) {
      return;
    }

    event.preventDefault();
    this.finishDrag();
  }

  /**
   * Touch end handler that finalises the drag.
   */
  private handleTouchEnd(event: TouchEvent) {
    if (!this.drag?.getActiveTouch(event.changedTouches)) {
      return;
    }

    event.preventDefault();
    this.finishDrag();
  }

  /**
   * Finish a drag session: reorder rows and apply indicators.
   */
  private finishDrag() {
    if (!this.drag) {
      return;
    }

    const drag = this.drag;
    const finalOrder = drag.getOrder(this.rows);

    const movedUp: HTMLTableRowElement[] = [];
    const movedDown: HTMLTableRowElement[] = [];

    finalOrder.forEach((row, index) => {
      // skip the dragged element
      if (row === drag.target) {
        return;
      }

      const previousIndex = this.rows.indexOf(row);
      if (previousIndex > index) {
        movedUp.push(row);
      } else if (previousIndex < index) {
        movedDown.push(row);
      }
    });

    const draggedRow = drag.target;

    finalOrder.forEach((row) => {
      row.style.transform = "";
      row.classList.remove("dragging");
      this.tbody.appendChild(row);
    });

    this.rows = finalOrder;

    this.drag = null;

    this.updatePositionLabels(this.rows);

    this.notifyChangeIfNeeded(drag.isDragging);

    if (draggedRow) {
      draggedRow.classList.add("flash-success");
      movedUp.forEach((row) => {
        this.applyIndicator(row, "up");
      });
      movedDown.forEach((row) => {
        this.applyIndicator(row, "down");
      });

      this.timeout = globalThis.setTimeout(() => {
        document
          .querySelectorAll(".flash-success, .pos-up, .pos-down")
          .forEach((el) => {
            el.classList.add("fade-out");
          });

        this.timeout = globalThis.setTimeout(() => {
          this.reset();
        }, 500);
      }, 2000);
    }
  }

  private reset() {
    if (this.timeout) {
      globalThis.clearTimeout(this.timeout);
      this.timeout = null;
    }

    document
      .querySelectorAll(".flash-success, .pos-up, .pos-down, .fade-out")
      .forEach((el) => {
        el.classList.remove("flash-success", "pos-up", "pos-down", "fade-out");
      });
  }

  /**
   * Schedule a frame to update drag transforms.
   */
  private scheduleDragUpdate() {
    if (!this.drag?.scheduleAnimation()) {
      return;
    }

    requestAnimationFrame(() => this.runDragUpdate());
  }

  /**
   * Apply one drag animation frame.
   */
  private runDragUpdate() {
    const drag = this.drag;
    if (!drag) {
      return;
    }

    drag.startFrame();

    const pendingY = drag.getPendingY();
    if (pendingY === null) {
      return;
    }

    const scrolled = this.autoScrollIfNeeded(pendingY);

    drag.markDragging();

    const { deltaY, currentTop, currentBottom } =
      drag.computeDragDelta(pendingY);
    drag.target.style.transform = `translate(0, ${deltaY}px)`;

    const { movedAbove, movedBelow } = drag.shiftOtherRows(
      this.rows,
      currentTop,
      currentBottom,
    );

    const compactOrder = drag.updateLiveOrder(
      this.rows,
      movedAbove,
      movedBelow,
    );
    this.updatePositionLabels(compactOrder);

    if (scrolled) {
      this.scheduleDragUpdate();
    }
  }

  /**
   * Scroll the viewport when dragging near its edges.
   */
  private autoScrollIfNeeded(pointerY: number) {
    const threshold = 40;
    const step = 15;
    const viewportHeight = globalThis.innerHeight;
    let scrolled = false;

    if (pointerY < threshold) {
      globalThis.scrollBy(0, -Math.min(step, threshold - pointerY));
      scrolled = true;
    } else if (pointerY > viewportHeight - threshold) {
      globalThis.scrollBy(
        0,
        Math.min(step, pointerY - (viewportHeight - threshold)),
      );
      scrolled = true;
    }

    return scrolled;
  }

  /**
   * Apply position change indicator styling to a row.
   */
  private applyIndicator(row: HTMLTableRowElement, direction: "up" | "down") {
    const cell = this.getPositionCell(row);
    if (!cell) {
      return;
    }

    cell.classList.remove("pos-up", "pos-down");
    cell.classList.add(direction === "up" ? "pos-up" : "pos-down");
  }

  /**
   * Cache and return the position cell for a row.
   */
  private getPositionCell(
    row: HTMLTableRowElement,
  ): HTMLTableCellElement | null {
    if (this.positionCellCache.has(row)) {
      return this.positionCellCache.get(row) ?? null;
    }

    const cells = Array.from(row.querySelectorAll<HTMLTableCellElement>("td"));
    const cell = cells[1] ?? null;
    if (cell) {
      this.positionCellCache.set(row, cell);
    }

    return cell;
  }

  /**
   * Update numeric position labels for the given ordering.
   */
  private updatePositionLabels(orderedRows: HTMLTableRowElement[]) {
    orderedRows.forEach((row, index) => {
      const cell = this.getPositionCell(row);
      if (!cell) {
        return;
      }

      const indicator = cell.querySelector(".position-badge");
      if (!indicator) {
        return;
      }

      const newValue = String(index + 1);
      if (indicator.textContent !== newValue) {
        indicator.textContent = newValue;
      }
    });
  }

  /**
   * Return the current row ordering as data-id values.
   */
  private getRowOrderIds(rows: HTMLTableRowElement[]) {
    const ids: string[] = [];

    for (const row of rows) {
      const id = row.dataset.id;
      if (!id) {
        return null;
      }
      ids.push(id);
    }

    return ids;
  }

  /**
   * Invoke the change callback when the order changes.
   */
  private notifyChangeIfNeeded(didDrag: boolean) {
    if (!this.onChange || !didDrag) {
      this.dragStartOrder = null;
      return;
    }

    const previous = this.dragStartOrder;
    const current = this.getRowOrderIds(this.rows);
    this.dragStartOrder = null;

    if (!previous || !current) {
      return;
    }

    if (previous.length !== current.length) {
      this.onChange(current);
      return;
    }

    const hasChanged = previous.some(
      (value, index) => value !== current[index],
    );
    if (hasChanged) {
      this.onChange(current);
    }
  }
}

// Helper function to send the new order to the server after a drag-and-drop
// interaction, as a JSON object with the row ids under `key`.
export function reorderList(updateUrl: string, key: string, ids: string[]) {
  // CSRF token from the layout's meta tag; the CSRF guard requires it.
  const csrfToken =
    document
      .querySelector('meta[name="csrf-token"]')
      ?.getAttribute("content") ?? "";
  fetch(updateUrl, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      "X-CSRF-Token": csrfToken,
    },
    body: JSON.stringify({ [key]: ids }),
  })
    .then((response) => {
      if (!response.ok) {
        console.error("Failed to update order", response.status);
      }
    })
    .catch((error) => {
      console.error("Failed to update order", error);
    });
}

// Public setup function to initialize sortable tables on the page.
export default function setupSortable() {
  const table: HTMLTableElement | null =
    document.querySelector("table.sortable");
  const tbody = table?.querySelector("tbody");

  if (!table || !tbody) {
    return null;
  }

  const updateUrl = table.dataset.sortableUpdateUrl;
  const updateKey = table.dataset.sortableUpdateKey ?? "person_ids";
  const sortable = new SortableTable(tbody);

  sortable.attachHandleEvents();
  sortable.attachGlobalEvents();

  if (updateUrl) {
    sortable.setOnChange((order: string[]) => {
      reorderList(updateUrl, updateKey, order);
    });
  }

  return sortable;
}
