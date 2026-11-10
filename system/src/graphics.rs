#![warn(dead_code)]
#![warn(non_snake_case)]
pub fn put_pixel(x: i32, y: i32, colour: u32) {
        // Ignore coordinates outside the display.
        if x < 0
            || y < 0
            || x >= GPU_WIDTH as i32
            || y >= GPU_HEIGHT as i32
        {
            return;
        }

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            let index = y as usize * GPU_WIDTH + x as usize;
            framebuffer.add(index).write_volatile(colour);
        }
    }
pub fn fill_rect(
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        colour: u32,
    ) {
        if width <= 0 || height <= 0 {
            return;
        }

        // Clip the rectangle against the framebuffer boundaries.
        let left = x.max(0);
        let top = y.max(0);
        let right = x.saturating_add(width).min(GPU_WIDTH as i32);
        let bottom = y.saturating_add(height).min(GPU_HEIGHT as i32);

        if left >= right || top >= bottom {
            return;
        }

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            for row in top..bottom {
                let start = row as usize * GPU_WIDTH + left as usize;
                let end = row as usize * GPU_WIDTH + right as usize;

                for index in start..end {
                    framebuffer.add(index).write_volatile(colour);
                }
            }
        }
    }
pub fn draw_rect(
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

        // Four filled rectangles make the border.
        Self::fill_rect(x, y, width, thickness, colour);
        Self::fill_rect(
            x,
            y.saturating_add(height).saturating_sub(thickness),
            width,
            thickness,
            colour,
        );
        Self::fill_rect(x, y, thickness, height, colour);
        Self::fill_rect(
            x.saturating_add(width).saturating_sub(thickness),
            y,
            thickness,
            height,
            colour,
        );
    }
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

            _ => [14, 17, 1, 2, 4, 0, 4], // Unknown character: ?
        }
    }
pub fn draw_char(x: i32, y: i32, character: char, colour: u32) {
        let rows = Self::glyph_rows(character);

        for (row_index, bits) in rows.iter().enumerate() {
            for column in 0..5 {
                // The top bit of each 5-bit row is the leftmost pixel.
                if bits & (1u8 << (4 - column)) != 0 {
                    Self::put_pixel(
                        x.saturating_add(column as i32),
                        y.saturating_add(row_index as i32),
                        colour,
                    );
                }
            }
        }
    }
pub fn draw_text(
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

            // Move to a new line if the next character would exceed the screen.
            if x.saturating_add(5) > GPU_WIDTH as i32 {
                x = line_start;
                y = y.saturating_add(9);
            }

            if y >= GPU_HEIGHT as i32 {
                break;
            }

            Self::draw_char(x, y, character, colour);
            x = x.saturating_add(6);
        }
    }
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

    pub fn draw(&mut self) {
        if self.drawn {
            self.erase();
        }

        self.x = self.x.clamp(
            0,
            GPU_WIDTH as i32 - CURSOR_WIDTH as i32,
        );
        self.y = self.y.clamp(
            0,
            GPU_HEIGHT as i32 - CURSOR_HEIGHT as i32,
        );

        self.paint();
    }

    pub fn move_by(&mut self, dx: i32, dy: i32) -> bool {
        if !self.drawn {
            self.draw();
        }

        let new_x = self.x.saturating_add(dx).clamp(
            0,
            GPU_WIDTH as i32 - CURSOR_WIDTH as i32,
        );
        let new_y = self.y.saturating_add(dy).clamp(
            0,
            GPU_HEIGHT as i32 - CURSOR_HEIGHT as i32,
        );

        if new_x == self.x && new_y == self.y {
            return false;
        }

        self.erase();

        self.x = new_x;
        self.y = new_y;

        self.paint();
        true
    }

    fn erase(&mut self) {
        use core::ptr::write_volatile;

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            for row in 0..CURSOR_HEIGHT {
                for col in 0..CURSOR_WIDTH {
                    let x = self.x + col as i32;
                    let y = self.y + row as i32;
                    let saved_index = row * CURSOR_WIDTH + col;

                    if x >= 0
                        && y >= 0
                        && x < GPU_WIDTH as i32
                        && y < GPU_HEIGHT as i32
                    {
                        let index =
                            y as usize * GPU_WIDTH + x as usize;

                        write_volatile(
                            framebuffer.add(index),
                            self.saved[saved_index],
                        );
                    }
                }
            }
        }

        self.drawn = false;
    }

    fn paint(&mut self) {
        use core::ptr::{read_volatile, write_volatile};

        unsafe {
            let framebuffer =
                core::ptr::addr_of_mut!(FRAMEBUFFER.0) as *mut u32;

            // Save the pixels beneath the cursor before drawing it.
            for row in 0..CURSOR_HEIGHT {
                for col in 0..CURSOR_WIDTH {
                    let x = self.x + col as i32;
                    let y = self.y + row as i32;
                    let saved_index = row * CURSOR_WIDTH + col;

                    self.saved[saved_index] =
                        if x >= 0
                            && y >= 0
                            && x < GPU_WIDTH as i32
                            && y < GPU_HEIGHT as i32
                        {
                            let index =
                                y as usize * GPU_WIDTH + x as usize;

                            read_volatile(framebuffer.add(index))
                        } else {
                            0
                        };
                }
            }

            // Draw the black outline and white interior.
            for row in 0..CURSOR_HEIGHT {
                let pattern = CURSOR_PATTERN[row].as_bytes();

                for col in 0..CURSOR_WIDTH {
                    let colour = match pattern[col] {
                        b'#' => 0xFF00_0000,
                        b'W' => 0xFFFF_FFFF,
                        _ => continue,
                    };

                    let x = self.x + col as i32;
                    let y = self.y + row as i32;

                    if x >= 0
                        && y >= 0
                        && x < GPU_WIDTH as i32
                        && y < GPU_HEIGHT as i32
                    {
                        let index =
                            y as usize * GPU_WIDTH + x as usize;

                        write_volatile(
                            framebuffer.add(index),
                            colour,
                        );
                    }
                }
            }
        }

        self.drawn = true;
        
    }
}