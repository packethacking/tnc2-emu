/* ************************************************************************
 *
 * Binary to hex converter for motorola format hex
 *
 * (C) By Dave Roberts G8KBB.
 * This program is provided 'as is', with no warranty. It has been
 * produced for use in self-tuition in amateur radio and is not
 * to be used for commercial applications. It may be freely copied and
 * used under these conditions.
 *
 * This version includes a bug fix attributable to KH6ILT
 *
 * Invocation :
 *
 *	MOTOROLA infile outfile [ hex_start_address ]
 *
 * Return codes : AS detailed in the source. 0 for success, error
 *                codes 1 - 5 errors as detailed below.
 *
 *************************************************************************/
 
#include <stdio.h>

unsigned Address;		/* effective address of data */
unsigned char Buffer[32786];	/* buffer for i/p data from file */
unsigned char *Ptr;		/* pointer into buffer for reading */
unsigned Count;			/* number of bytes left in buffer */
FILE *Fpin, *Fpout;		/* input & output file handles */

/* ***********************************************************************
 * this is the main program.
 * It exits with an error code of 0 if all went correctly.
 * With 1 for a syntax error
 *      2 for an input file open error
 *      3 for an output file open error
 *      4 for an input file read error
 *      5 for an output file write error
 * **********************************************************************/

main( argc, argv )
char *argv[];
{
	int ecode;		/* error code on exit from main loop */
	char *tmpptr;		/* scratch pointer for strtol() */
	int cont;		/* flag to say stop looping */

	Address = 0;		/* address is zero unless user inputs one */
	Count = 0;		/* mark buffer as empty */
	ecode = 0;		/* error code will be zero if all goes well */
	Ptr = Buffer;		/* point to start of buffer */
	cont = 1;		/* flag that makes main loop loop ! */

	printf("Motorola S record dump program, Version 2.0\n\n");

	/* we need an input file & an output file.
	 * A start address is optional
	 * so need 2 or 3 parameters !
	 */
	if( argc < 3 || argc > 4 )
	{
		printf( "Error - usage is :\n    motorola infile outfile [ start-address ]\n");
		exit( 1 );
	}

	/* OK, we seem to have the parameters - so let's open the input
	 * file in BINARY !!!! mode.
	 * Error & exit if cannot.
	 */
	if( ( Fpin = fopen( argv[1], "rb"  ) ) == NULL )
	{
		perror("Input file error ");
		exit( 2 );
	}

	/* Next, lets open the output file for write, in default mode
	 * Error & exit if cannot
	 */
	if( ( Fpout = fopen( argv[2], "w" ) ) == NULL )
	{
		perror("Output file error ");
		exit( 3 );
	}

	/* Now the optional start address.
	 * WARNING - being a lazy sod, just take the last 16 bits
	 */
	if( argc == 4 )
		Address = strtol( argv[3], &tmpptr, 16 );

	/* output the file header record
	 */
	fprintf( Fpout, "S00600004844521B\n" );

	/* MAIN LOOP - keep doing this until there is no more data
	 * or until an error occurs.
	 *
	 * Read a buffer full if there is less than 16 bytes left in buffer
	 * Crash out if there is a read error.
	 * Dump either 16 bytes, or whatever is left to dump
	 * Crash out if write error
	 * Exit anyway if there was less than 16 bytes ( as we did not 
	 * read any more
	 */
	while( cont )
	{
		if( Count < 16 )
		{
			if( !getdata() )
			{
				perror("Input file ");
				ecode = 4;
				break;
			}
		}
		if( Count < 16 )
			cont = 0;
		if( !dump( Count >= 16 ? 16 : Count ) )
		{
			perror("Output file ");
			ecode = 5;
			break;
		}
	}

	/* dump trailing line
	 * close files
	 * and exit.
	 */
	fprintf( Fpout, "S9030000FC\n" );
	fclose( Fpin );
	fclose( Fpout );
	return( ecode );
}

/* ***********************************************************************
 * This function reads data from the input file.
 * Firstly, if there is still a bit of data left in the buffer, move
 * it down to the bottom of the buffer.
 * Then point to the start of the buffer so we handle that data first.
 * Now try to fill the buffer up.
 * If we read less than we expected, just check to see that the reason
 * was end of file & if not die horribly.
 * Now check for file read errors for additional ways to die.
 * Finally, update the byte counter to reflect the new buffer size & return
 * a return of 0 means error, 1 means all went well.
 * ***********************************************************************/
getdata()
{
	unsigned int i;		/* temp store used for bytes actually read */

	if( Count != 0 )
		memmove( Buffer, Ptr, Count );
	Ptr = Buffer;
	i = fread( Buffer+Count, sizeof( char ), sizeof(Buffer)-Count, Fpin );
	if( i != sizeof(Buffer)-Count && !feof( Fpin ) )
		return( 0 );
	if( ferror( Fpin ) )
		return( 0 );
	Count += i;
	return( 1 );
}

/* ************************************************************************
 * Dump function for a line of data.
 * This outputs a single line of (num) bytes of data.
 * It checks each write for an error.
 * Firstly, it sends the line preamble, count, address and type.
 * Then it sends each byte in turn.
 * Finally it computes the checksum & sends that too.
 * A return of 0 means error, a return of 1 means all went well.
 * ***********************************************************************/
dump( num )
unsigned num;
{
	int i;			/* counter for number of bytes processed */
	unsigned char csum;	/* checksum value for line of data */

	if( num == 0 )
		return( 1 );
	fprintf( Fpout, "S1%02X%04X", num + 3, Address );
	if( ferror( Fpout ) )
		return( 0 );
	csum = num + 3 + ( (Address >> 8) & 0xff) + (Address & 0xff);
	for( i=0; i<num; i++ )
	{
		fprintf( Fpout, "%02X", *Ptr );
		if( ferror( Fpout ) )
			return( 0 );
		csum += *Ptr++;
		Count--;
		if( ! ++Address )
			printf("WARNING. The address exceeded 0xffff.\n");
	}
	fprintf( Fpout, "%02X\n", (~csum) & 0xff );
	if( ferror( Fpout ) )
		return( 0 );
	return( 1 );
}
