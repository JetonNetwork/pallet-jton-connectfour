#![cfg_attr(not(feature = "std"), no_std)]

/// Edit this file to define custom logic or remove it if it is not needed.
/// Learn more about FRAME and the core library of Substrate FRAME pallets:
/// <https://substrate.dev/docs/en/knowledgebase/runtime/frame>

use codec::{Encode, Decode};
use frame_support::{
	log,
	traits::{Randomness},
};
use scale_info::TypeInfo;
use sp_runtime::{
	traits::{Hash, TrailingZeroInput}
};
use sp_std::vec::{
	Vec
};

// Re-export pallet items so that they can be accessed from the crate namespace.
pub use pallet::*;

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

/// Implementations of some helper traits passed into runtime modules as associated types.
pub mod connectfour;
use connectfour::{Logic};

#[derive(Encode, Decode, Clone, PartialEq, TypeInfo)]
pub enum BoardState<AccountId> {
	None,
	Running,
	Finished(AccountId),
}

impl<AccountId> Default for BoardState<AccountId> { fn default() -> Self { Self::None } }

/// Connect four board structure containing two players and the board
#[derive(Encode, Decode, Default, Clone, PartialEq, TypeInfo)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct BoardStruct<Hash, AccountId, BlockNumber, BoardState> {
	id: Hash,
	red: AccountId,
	blue: AccountId,
	board: [[u8; 6]; 7],
	last_turn: BlockNumber,
	next_player: u8,
	board_state: BoardState,
}

const PLAYER_1: u8 = 1;
const PLAYER_2: u8 = 2;

#[frame_support::pallet]
pub mod pallet {
	use frame_support::{dispatch::DispatchResult, pallet_prelude::*};
	use frame_system::pallet_prelude::*;

	// important to use outside structs and consts
	use super::*;

	/// Configure the pallet by specifying the parameters and types on which it depends.
	#[pallet::config]
	pub trait Config: frame_system::Config {
		/// Because this pallet emits events, it depends on the runtime's definition of an event.
		type Event: From<Event<Self>> + IsType<<Self as frame_system::Config>::Event>;

		/// The generator used to supply randomness to contracts through `seal_random`.
		type Randomness: Randomness<Self::Hash, Self::BlockNumber>;
	}

	#[pallet::pallet]
	#[pallet::generate_store(pub(super) trait Store)]
	pub struct Pallet<T>(_);

	#[pallet::storage]
	#[pallet::getter(fn boards)]
	/// Store all boards that are currently being played.
	pub type Boards<T: Config> = StorageMap<_, Identity, T::Hash, BoardStruct<T::Hash, T::AccountId, T::BlockNumber, BoardState<T::AccountId>>, ValueQuery>;

	#[pallet::storage]
	#[pallet::getter(fn player_board)]
	/// Store players active board, currently only one board per player allowed.
	pub type PlayerBoard<T: Config> = StorageMap<_, Identity, T::AccountId, T::Hash, ValueQuery>;

	// Default value for Nonce
	#[pallet::type_value]
	pub fn NonceDefault<T: Config>() -> u64 { 0 }
	// Nonce used for generating a different seed each time.
	#[pallet::storage]
	pub type Nonce<T: Config> = StorageValue<_, u64, ValueQuery, NonceDefault<T>>;

	// Pallets use events to inform users when important changes are made.
	// https://substrate.dev/docs/en/knowledgebase/runtime/events
	#[pallet::event]
	#[pallet::generate_deposit(pub(super) fn deposit_event)]
	pub enum Event<T: Config> {
		/// A new board got created.
		NewBoard(T::Hash),
	}

	// Errors inform users that something went wrong.
	#[pallet::error]
	pub enum Error<T> {
		/// Player already has a board which is being played.
		PlayerBoardExists,
		/// Player board doesn't exist for this player.
		NoPlayerBoard,
		/// Player can't play against them self.
		NoFakePlay,
		/// Wrong player for next turn.
		NotPlayerTurn,
		/// There was an error while trying to execute something in the logic mod.
		WrongLogic,
	}

	#[pallet::hooks]
	impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
	}

	#[pallet::call]
	impl<T:Config> Pallet<T> {

		/// Create game for two players
		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn new_game(origin: OriginFor<T>, opponent: T::AccountId) -> DispatchResult {
			
			let sender = ensure_signed(origin)?;
			// Don't allow playing against yourself.
			ensure!(sender != opponent, Error::<T>::NoFakePlay);
		
			// Make sure players have no board open.
			ensure!(!PlayerBoard::<T>::contains_key(&sender), Error::<T>::PlayerBoardExists);
			ensure!(!PlayerBoard::<T>::contains_key(&opponent), Error::<T>::PlayerBoardExists);
					
			// Create new game
			let board_id = Self::create_game(sender.clone(), opponent.clone());
		
			// Add board to the players playing it.
			<PlayerBoard<T>>::insert(sender, board_id);
			<PlayerBoard<T>>::insert(opponent, board_id);
		
			Ok(())
		}

		/// play turn by player
		#[pallet::weight(10_000 + T::DbWeight::get().reads_writes(1,1))]
		pub fn play_turn(origin: OriginFor<T>, column: u8) -> DispatchResult {
			
			let sender = ensure_signed(origin)?;

			ensure!(column < 8, "Game only allows columns smaller then 8");

			// TODO: should PlayerBoard storage here be optional to avoid two reads?
			ensure!(PlayerBoard::<T>::contains_key(&sender), Error::<T>::NoPlayerBoard);
			let board_id = Self::player_board(&sender);
	
			// Get board from player.
			ensure!(Boards::<T>::contains_key(&board_id), "No board found");
			let mut board = Self::boards(&board_id);
			
			// Board is still open to play and not finished.
			ensure!(board.board_state == BoardState::Running, "Board is not running, check if already finished.");

			let current_player = board.next_player;
			let current_account;

			// Check if correct player is at turn
			if current_player == PLAYER_1 {
				current_account = board.red.clone();
				board.next_player = PLAYER_2;
			} else if current_player == PLAYER_2 {
				current_account = board.blue.clone();
				board.next_player = PLAYER_1;
			} else {
				return Err(Error::<T>::WrongLogic)?
			}

			// Make sure current account is at turn.
			ensure!(sender == current_account, Error::<T>::NotPlayerTurn);

			// Check if we can successfully place a stone in that column
			if !Logic::add_stone(&mut board.board, column, current_player) {
				return Err(Error::<T>::WrongLogic)?
			}

			// Check if the last played stone gave us a winner or board is full
			if Logic::evaluate(board.board.clone(), current_player) {
				board.board_state = BoardState::Finished(current_account);
			} else if Logic::full(board.board.clone()) {
				board.board_state = BoardState::Finished(Default::default());
			}

			// get current blocknumber
			let last_turn = <frame_system::Pallet<T>>::block_number();
			board.last_turn = last_turn;

			// Write next board state back into the storage
			<Boards<T>>::insert(board_id, board);

			Ok(())
		}

	}
}

impl<T: Config> Pallet<T> {
	/// Update nonce once used. 
	fn encode_and_update_nonce(
	) -> Vec<u8> {
		let nonce = <Nonce<T>>::get();
		<Nonce<T>>::put(nonce.wrapping_add(1));
		nonce.encode()
	}

	/// Generates a random hash out of a seed.
	fn generate_random_hash(
		phrase: &[u8], 
		sender: T::AccountId
	) -> T::Hash {
		let (seed, _) = T::Randomness::random(phrase);
		let seed = <[u8; 32]>::decode(&mut TrailingZeroInput::new(seed.as_ref()))
			.expect("input is padded with zeroes; qed");
		return (seed, &sender, Self::encode_and_update_nonce()).using_encoded(T::Hashing::hash);
	}

	/// Generate a new game between two players.
	fn create_game(
		red: T::AccountId, 
		blue: T::AccountId
	) -> T::Hash {
		// get a random hash as board id
		let board_id = Self::generate_random_hash(b"create", red.clone());
		// calculate plyer to start the first turn, with the first byte of the board_id random hash
		let next_player = if board_id.as_ref()[0] < 128 { PLAYER_1 } else { PLAYER_2 };
		// get current blocknumber
		let block_number = <frame_system::Pallet<T>>::block_number();
		// create a new empty bgame oard
		let board = BoardStruct {
			id: board_id,
			red: red,
			blue: blue,
			board: [[0u8; 6]; 7],
			last_turn: block_number,
			next_player: next_player,
			board_state: BoardState::Running,
		};
		// insert the new board into the storage
		<Boards<T>>::insert(board_id, board);
		// emit event for a new board creation
		// Emit an event.
		Self::deposit_event(Event::NewBoard(board_id));

		return board_id;
	}
}

