
#![warn(dead_code)]
#![warn(non_snake_case)]

/// A rectangular drawing area backed by a pixel buffer.
///
/// Each pixel is a 32-bit colour value.
/// The buffer is owned by the caller; Surface only borrows it.
pub struct Surface<'a> {
    width: usize,
    height: usize,
    stride: usize,
    pixels: &'a mut [u32],
}

impl<'a> Surface<'a> {
    /// Create a new surface.
    ///
    /// Returns None if the dimensions are invalid or the
    /// supplied pixel buffer is too small.
    pub fn new(
        width: usize,
        height: usize,
        pixels: &'a mut [u32],
    ) -> Option<Self> {
        if width == 0 || height == 0 {
            return None;
        }

        let required = width.checked_mul(height)?;

        if pixels.len() < required {
            return None;
        }

        Some(Self {
            width,
            height,
            stride: width,
            pixels,
        })
    }

    /// Return the surface width.
    pub fn width(&self) -> usize {
        self.width
    }

    /// Return the surface height.
    pub fn height(&self) -> usize {
        self.height
    }

    /// Return the number of pixels between rows.
    pub fn stride(&self) -> usize {
        self.stride
    }

    /// Set one pixel.
    ///
    /// Coordinates outside the surface are ignored.
    pub fn put_pixel(
        &mut self,
        x: i32,
        y: i32,
        colour: u32,
    ) {
        if x < 0 || y < 0 {
            return;
        }

        let x = x as usize;
        let y = y as usize;

        if x >= self.width || y >= self.height {
            return;
        }

        let index = y * self.stride + x;
        self.pixels[index] = colour;
    }

    /// Read one pixel.
    ///
    /// Returns None if the coordinates are outside the surface.
    pub fn get_pixel(
        &self,
        x: i32,
        y: i32,
    ) -> Option<u32> {
        if x < 0 || y < 0 {
            return None;
        }

        let x = x as usize;
        let y = y as usize;

        if x >= self.width || y >= self.height {
            return None;
        }

        Some(self.pixels[y * self.stride + x])
    }

    /// Fill a rectangular area.
    ///
    /// Rectangles are clipped against the surface boundaries.
    pub fn fill_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        colour: u32,
    ) {
        if width <= 0 || height <= 0 {
            return;
        }

        let left = (x as i64).max(0);
        let top = (y as i64).max(0);

        let right = (x as i64 + width as i64)
            .min(self.width as i64);

        let bottom = (y as i64 + height as i64)
            .min(self.height as i64);

        if left >= right || top >= bottom {
            return;
        }

        for row in top as usize..bottom as usize {
            let start = row * self.stride + left as usize;
            let end = row * self.stride + right as usize;

            self.pixels[start..end].fill(colour);
        }
    }

    /// Draw a rectangular outline.
    pub fn draw_rect(
        &mut self,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        thickness: i32,
        colour: u32,
    ) {
        if width <= 0 || height <= 0 || thickness <= 0 {
            return;
        }

        let thickness = thickness.min(width).min(height);

        self.fill_rect(
            x,
            y,
            width,
            thickness,
            colour,
        );

        self.fill_rect(
            x,
            y.saturating_add(height).saturating_sub(thickness),
            width,
            thickness,
            colour,
        );

        self.fill_rect(
            x,
            y,
            thickness,
            height,
            colour,
        );

        self.fill_rect(
            x.saturating_add(width).saturating_sub(thickness),
            y,
            thickness,
            height,
            colour,
        );
    }

    /// Get the 5x7 bitmap for a character.
    fn glyph_rows(character: char) -> [u8; 7] {
        match character.to_ascii_uppercase() {
            'A' => [14, 17, 17, 31, 17, 17, 17],
            'B' => [30, 17, 17, 30, 17, 17, 30],
            'C' => [14, 17, 16, 16, 16, 17, 14],
            'D' => [30, 17, 17, 17, 17, 17, 30],
            'E' => [31, 16, 16, 30, 16, 16, 31],
            'F' => [31, 16, 16, 30, 16, 16, 16],
            'G' => [14, 17, 16, 23, 17, 17, 15],
            'H' => [17, 17, 17, 31, 17, 17, 17],
            'I' => [14, 4, 4, 4, 4, 4, 14],
            'J' => [7, 2, 2, 2, 18, 18, 12],
            'K' => [17, 18, 20, 24, 20, 18, 17],
            'L' => [16, 16, 16, 16, 16, 16, 31],
            'M' => [17, 27, 21, 21, 17, 17, 17],
            'N' => [17, 25, 21, 19, 17, 17, 17],
            'O' => [14, 17, 17, 17, 17, 17, 14],
            'P' => [30, 17, 17, 30, 16, 16, 16],
            'Q' => [14, 17, 17, 17, 21, 18, 13],
            'R' => [30, 17, 17, 30, 20, 18, 17],
            'S' => [15, 16, 16, 14, 1, 1, 30],
            'T' => [31, 4, 4, 4, 4, 4, 4],
            'U' => [17, 17, 17, 17, 17, 17, 14],
            'V' => [17, 17, 17, 17, 17, 10, 4],
            'W' => [17, 17, 17, 21, 21, 21, 10],
            'X' => [17, 17, 10, 4, 10, 17, 17],
            'Y' => [17, 17, 10, 4, 4, 4, 4],
            'Z' => [31, 1, 2, 4, 8, 16, 31],

            '0' => [14, 17, 19, 21, 25, 17, 14],
            '1' => [4, 12, 4, 4, 4, 4, 14],
            '2' => [14, 17, 1, 2, 4, 8, 31],
            '3' => [30, 1, 1, 14, 1, 1, 30],
            '4' => [2, 6, 10, 18, 31, 2, 2],
            '5' => [31, 16, 16, 30, 1, 1, 30],
            '6' => [14, 16, 16, 30, 17, 17, 14],
            '7' => [31, 1, 2, 4, 8, 8, 8],
            '8' => [14, 17, 17, 14, 17, 17, 14],
            '9' => [14, 17, 17, 15, 1, 1, 14],

            ' ' => [0; 7],
            '.' => [0, 0, 0, 0, 0, 4, 4],
            ',' => [0, 0, 0, 0, 4, 4, 8],
            ':' => [0, 4, 4, 0, 4, 4, 0],
            '!' => [4, 4, 4, 4, 4, 0, 4],
            '?' => [14, 17, 1, 2, 4, 0, 4],
            '-' => [0, 0, 0, 31, 0, 0, 0],
            '_' => [0, 0, 0, 0, 0, 0, 31],
            '/' => [1, 2, 2, 4, 8, 8, 16],
            '+' => [0, 4, 4, 31, 4, 4, 0],
            '=' => [0, 31, 0, 31, 0, 0, 0],
            '(' => [2, 4, 8, 8, 8, 4, 2],
            ')' => [8, 4, 2, 2, 2, 4, 8],
            '\'' => [4, 4, 8, 0, 0, 0, 0],

            _ => [14, 17, 1, 2, 4, 0, 4],
        }
    }

    /// Draw one 5x7 character.
    pub fn draw_char(
        &mut self,
        x: i32,
        y: i32,
        character: char,
        colour: u32,
    ) {
        let rows = Self::glyph_rows(character);

        for (row_index, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                if bits & (1u8 << (4 - column)) != 0 {
                    self.put_pixel(
                        x.saturating_add(column as i32),
                        y.saturating_add(row_index as i32),
                        colour,
                    );
                }
            }
        }
    }

    /// Draw text.
    ///
    /// Characters are 5x7 pixels with one pixel of horizontal spacing
    /// and two pixels of vertical spacing.
    pub fn draw_text(
        &mut self,
        mut x: i32,
        mut y: i32,
        text: &str,
        colour: u32,
    ) {
        let line_start = x;

        for character in text.chars() {
            if character == '\n' {
                x = line_start;
                y = y.saturating_add(9);
                continue;
            }

            if x.saturating_add(5) > self.width as i32 {
                x = line_start;
                y = y.saturating_add(9);
            }

            if y >= self.height as i32 {
                break;
            }

            self.draw_char(x, y, character, colour);
            x = x.saturating_add(6);
        }
    }
}

// -----------------------------------------------------------------------------
// Mouse cursor
// -----------------------------------------------------------------------------
//
// The cursor is also drawn onto a Surface.
// It has no knowledge of the display or GPU.

const CURSOR_WIDTH: usize = 16;
const CURSOR_HEIGHT: usize = 16;

const CURSOR_PATTERN: [&str; CURSOR_HEIGHT] = [
    "#...............",
    "##..............",
    "#W#.............",
    "#WW#............",
    "#WWW#...........",
    "#WWWW#..........",
    "#WWWWW#.........",
    "#WWWWWW#........",
    "#WWWWWWW#.......",
    "#WWWW#####......",
    "#WWW#...........",
    "#WW#.#..........",
    "#W#..#..........",
    "##....#.........",
    "#.....#.........",
    "......##........",
];

pub struct MouseCursor {
    x: i32,
    y: i32,
    saved: [u32; CURSOR_WIDTH * CURSOR_HEIGHT],
    drawn: bool,
}

impl MouseCursor {
    pub const fn new(x: i32, y: i32) -> Self {
        Self {
            x,
            y,
            saved: [0; CURSOR_WIDTH * CURSOR_HEIGHT],
            drawn: false,
        }
    }

    /// Draw the cursor onto a surface.
    pub fn draw(&mut self, surface: &mut Surface<'_>) {
        if self.drawn {
            self.erase(surface);
        }

        self.clamp_position(surface);
        self.paint(surface);
    }

    /// Move the cursor by a relative amount.
    ///
    /// Returns false if the cursor did not move.
    pub fn move_by(
        &mut self,
        dx: i32,
        dy: i32,
        surface: &mut Surface<'_>,
    ) -> bool {
        if !self.drawn {
            self.draw(surface);
        }

        let old_x = self.x;
        let old_y = self.y;

        self.x = self.x.saturating_add(dx);
        self.y = self.y.saturating_add(dy);

        self.clamp_position(surface);

        if self.x == old_x && self.y == old_y {
            return false;
        }

        self.erase(surface);
        self.paint(surface);

        true
    }

    /// Clamp the cursor to the surface boundaries.
    fn clamp_position(&mut self, surface: &Surface<'_>) {
        let max_x = surface.width.saturating_sub(CURSOR_WIDTH) as i32;
        let max_y = surface.height.saturating_sub(CURSOR_HEIGHT) as i32;

        self.x = self.x.clamp(0, max_x.max(0));
        self.y = self.y.clamp(0, max_y.max(0));
    }

    /// Restore the pixels underneath the cursor.
    fn erase(&mut self, surface: &mut Surface<'_>) {
        for row in 0..CURSOR_HEIGHT {
            for col in 0..CURSOR_WIDTH {
                let x = self.x + col as i32;
                let y = self.y + row as i32;

                let saved_index = row * CURSOR_WIDTH + col;

                surface.put_pixel(
                    x,
                    y,
                    self.saved[saved_index],
                );
            }
        }

        self.drawn = false;
    }

    /// Save the background and draw the cursor.
    fn paint(&mut self, surface: &mut Surface<'_>) {
        // Save the pixels underneath the cursor.
        for row in 0..CURSOR_HEIGHT {
            for col in 0..CURSOR_WIDTH {
                let x = self.x + col as i32;
                let y = self.y + row as i32;

                let saved_index = row * CURSOR_WIDTH + col;

                self.saved[saved_index] =
                    surface.get_pixel(x, y).unwrap_or(0);
            }
        }

        // Draw the cursor.
        for row in 0..CURSOR_HEIGHT {
            let pattern = CURSOR_PATTERN[row].as_bytes();

            for col in 0..CURSOR_WIDTH {
                let colour = match pattern[col] {
                    b'#' => 0xFF00_0000,
                    b'W' => 0xFFFF_FFFF,
                    _ => continue,
                };

                surface.put_pixel(
                    self.x + col as i32,
                    self.y + row as i32,
                    colour,
                );
            }
        }

        self.drawn = true;
    }
}
