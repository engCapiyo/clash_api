use std::io;

fn main() {
    println!("=== Simple Rust Calculator ===");

    loop {
        println!("\nOperations available:");
        println!("1. Addition (+)");
        println!("2. Subtraction (-)");
        println!("3. Multiplication (*)");
        println!("4. Division (/)");
        println!("5. Exit");

        print!("Choose an operation (1-5): ");
        io::Write::flush(&mut io::stdout()).unwrap();

        let mut choice = String::new();
        io::stdin().read_line(&mut choice).expect("Failed to read line");

        let choice: u32 = match choice.trim().parse() {
            Ok(num) => num,
            Err(_) => {
                println!("Please enter a valid number!");
                continue;
            }
        };

        if choice == 5 {
            println!("Goodbye!");
            break;
        }

        if choice < 1 || choice > 5 {
            println!("Please choose between 1-5!");
            continue;
        }

        // Get numbers from user
        let (num1, num2) = get_numbers();

        // Perform calculation
        match choice {
            1 => println!("Result: {} + {} = {}", num1, num2, add(num1, num2)),
            2 => println!("Result: {} - {} = {}", num1, num2, subtract(num1, num2)),
            3 => println!("Result: {} * {} = {}", num1, num2, multiply(num1, num2)),
            4 => {
                if num2 == 0.0 {
                    println!("Error: Cannot divide by zero!");
                } else {
                    println!("Result: {} / {} = {}", num1, num2, divide(num1, num2));
                }
            },
            _ => println!("Invalid operation!"),
        }
    }
}

fn get_numbers() -> (f64, f64) {
    println!("Enter first number: ");
    let num1 = get_number();

    println!("Enter second number: ");
    let num2 = get_number();

    (num1, num2)
}

fn get_number() -> f64 {
    loop {
        let mut input = String::new();
        io::stdin().read_line(&mut input).expect("Failed to read line");

        match input.trim().parse() {
            Ok(num) => return num,
            Err(_) => println!("Please enter a valid number!"),
        }
    }
}

// Math operations
fn add(a: f64, b: f64) -> f64 {
    a + b
}

fn subtract(a: f64, b: f64) -> f64 {
    a - b
}

fn multiply(a: f64, b: f64) -> f64 {
    a * b
}

fn divide(a: f64, b: f64) -> f64 {
    a / b
}